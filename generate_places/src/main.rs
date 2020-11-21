use indicatif::{ProgressBar, ProgressIterator, ProgressStyle};
use log::*;
use osmpbf::{Element, ElementReader};
use rand::prelude::*;
use simple_process_stats::ProcessStats;
use std::collections::{HashMap, HashSet};
use std::io::prelude::*;

#[derive(PartialEq, Debug)]
struct Location {
    latitude: f32,
    longitude: f32,
}

fn deg2rad(x: f32) -> f32 {
    3.14159265 * x / 180.0
}

fn rad2deg(x: f32) -> f32 {
    180.0 * x / 3.14159265
}

impl Location {
    fn new(lat: f64, lon: f64) -> Location {
        Location {
            latitude: lat as f32,
            longitude: lon as f32,
        }
    }

    fn angular_distance(&self, other: &Location) -> f32 {
        let lat_diff_sin = ((deg2rad(self.latitude) - deg2rad(other.latitude)) / 2.).sin();
        let lon_diff_sin = ((deg2rad(self.longitude) - deg2rad(other.longitude)) / 2.).sin();
        let h = lat_diff_sin * lat_diff_sin
            + deg2rad(self.latitude).cos()
                * deg2rad(other.latitude).cos()
                * lon_diff_sin
                * lon_diff_sin;
        2.0 * h.sqrt().asin()
    }

    fn distance_mm(&self, other: &Location) -> u64 {
        let lat_diff_sin = ((deg2rad(self.latitude) - deg2rad(other.latitude)) / 2.).sin();
        let lon_diff_sin = ((deg2rad(self.longitude) - deg2rad(other.longitude)) / 2.).sin();
        let h = lat_diff_sin * lat_diff_sin
            + deg2rad(self.latitude).cos()
                * deg2rad(other.latitude).cos()
                * lon_diff_sin
                * lon_diff_sin;
        let r = 6360.0;
        let d_km = 2.0 * r * h.sqrt().asin();
        (d_km * 1000000.0).floor() as u64
    }

    fn lerp(&self, alpha: f32, other: &Location) -> Location {
        let lat1 = deg2rad(self.latitude);
        let lat2 = deg2rad(other.latitude);
        let lon1 = deg2rad(self.longitude);
        let lon2 = deg2rad(other.longitude);

        let angular_distance = self.angular_distance(other);
        let a = ((1.0 - alpha) * angular_distance).sin() / angular_distance.sin();
        let b = (alpha * angular_distance).sin() / angular_distance.sin();
        let x = a * lat1.cos() * lon1.cos() + b * lat2.cos() * lon2.cos();
        let y = a * lat1.cos() * lon1.sin() + b * lat2.cos() * lon2.sin();
        let z = a * lat1.sin() + b * lat2.sin();
        let lat3 = z.atan2((x * x + y * y).sqrt());
        let lon3 = y.atan2(x);
        Location {
            latitude: rad2deg(lat3),
            longitude: rad2deg(lon3),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn loc_equal(a: &Location, b: &Location) {
        let distance = a.distance_mm(b);
        if distance > 100000 {
            assert_eq!(a, b);
        }
    }

    #[test]
    fn test_lerp() {
        let austin = Location {
            latitude: 30.266666,
            longitude: -97.733330,
        };
        let newyork = Location {
            latitude: 40.730610,
            longitude: -73.935242,
        };
        loc_equal(&austin.lerp(0.0, &newyork), &austin);
        loc_equal(&austin.lerp(1.0, &newyork), &newyork);
        loc_equal(&newyork.lerp(0.0, &austin), &newyork);
        loc_equal(&newyork.lerp(1.0, &austin), &austin);
        loc_equal(
            &newyork.lerp(0.5, &austin),
            &Location {
                latitude: 36.08647,
                longitude: -86.622765,
            },
        );
        loc_equal(
            &austin.lerp(0.5, &newyork),
            &Location {
                latitude: 36.08647,
                longitude: -86.622765,
            },
        );
    }
}

trait PointWriter {
    fn write(&mut self, location: &Location);
}

struct FilePointWriter {
    writer: std::io::BufWriter<std::fs::File>,
}

impl FilePointWriter {
    fn new(filename: &str) -> FilePointWriter {
        let f = std::fs::File::create(filename).unwrap();
        FilePointWriter {
            writer: std::io::BufWriter::new(f),
        }
    }
}

impl PointWriter for FilePointWriter {
    fn write(&mut self, location: &Location) {
        let lat = location.latitude.to_le_bytes();
        let lon = location.longitude.to_le_bytes();
        self.writer.write_all(&lat).unwrap();
        self.writer.write_all(&lon).unwrap();
    }
}

struct KvNodeExtractor {
    filter_key: String,
    filter_value: Option<String>,
    nodes: Vec<Location>,

    // Nodes that we need to lookup b/c they're part of a way
    node_ids: HashSet<i64>,
}

impl KvNodeExtractor {
    fn new(key: &str, value: Option<&str>) -> KvNodeExtractor {
        KvNodeExtractor {
            filter_key: key.to_string(),
            filter_value: match value {
                Some(s) => Some(s.to_string()),
                None => None,
            },
            nodes: vec![],
            node_ids: HashSet::new(),
        }
    }

    fn does_tag_match(&self, (k, v): (&str, &str)) -> bool {
        if k == self.filter_key {
            match &self.filter_value {
                Some(s) => {
                    return s == v;
                }
                None => {
                    return true;
                }
            }
        }
        return false;
    }

    fn process(&mut self, element: &Element) {
        match element {
            Element::Node(node) => {
                for tag in node.tags() {
                    if self.does_tag_match(tag) {
                        self.nodes.push(Location::new(node.lat(), node.lon()));
                    }
                }
            }
            Element::DenseNode(node) => {
                for tag in node.tags() {
                    if self.does_tag_match(tag) {
                        self.nodes.push(Location::new(node.lat(), node.lon()));
                    }
                }
            }
            Element::Way(way) => {
                // Sometimes they're a way representing the boundary
                for tag in way.tags() {
                    if self.does_tag_match(tag) {
                        self.node_ids.insert(way.refs().next().unwrap());
                    }
                }
            }
            Element::Relation(_) => {}
        }
    }

    fn second_pass(&mut self, element: &Element) {
        match element {
            Element::Node(node) => {
                if self.node_ids.contains(&node.id()) {
                    self.nodes.push(Location::new(node.lat(), node.lon()));
                }
            }
            Element::DenseNode(node) => {
                if self.node_ids.contains(&node.id) {
                    self.nodes.push(Location::new(node.lat(), node.lon()));
                }
            }
            Element::Way(_) => {}
            Element::Relation(_) => {}
        }
    }

    fn export(&self, mut writer: impl PointWriter) {
        for node in self.nodes.iter() {
            writer.write(node);
        }
    }
}

struct RoadExtractor {
    node_ids: HashSet<i64>,
    nodes: HashMap<i64, Location>,
    roads: Vec<Vec<i64>>,
}

impl RoadExtractor {
    fn new() -> RoadExtractor {
        RoadExtractor {
            node_ids: HashSet::new(),
            nodes: HashMap::new(),
            roads: vec![],
        }
    }

    fn first_pass(&mut self, element: &Element) {
        match element {
            Element::Node(_) => {}
            Element::DenseNode(_) => {}
            Element::Way(way) => {
                for (k, _) in way.tags() {
                    if k == "highway" {
                        let mut rng = rand::thread_rng();
                        if rng.gen::<f64>() > 0.01 {
                            // Only keep 1 in 100 roads
                            return;
                        }

                        let mut road = vec![];
                        for nodeid in way.refs() {
                            self.node_ids.insert(nodeid);
                            road.push(nodeid);
                        }
                        self.roads.push(road);
                    }
                }
            }
            Element::Relation(_) => {}
        }
    }

    fn second_pass(&mut self, element: &Element) {
        match element {
            Element::Node(node) => {
                if self.node_ids.contains(&node.id()) {
                    self.nodes
                        .insert(node.id(), Location::new(node.lat(), node.lon()));
                }
            }
            Element::DenseNode(node) => {
                if self.node_ids.contains(&node.id) {
                    self.nodes
                        .insert(node.id, Location::new(node.lat(), node.lon()));
                }
            }
            Element::Way(_) => {}
            Element::Relation(_) => {}
        }
    }

    fn export(&self, mut writer: impl PointWriter) {
        // First, compute the total length of all roads
        info!("Computing road lengths for {} roads...", self.roads.len());
        info!("Have {} nodes as parts of roads", self.node_ids.len());
        let mut total_length = 0;
        let mut road_lengths = vec![];
        let mut distance_so_far = vec![];
        for road in self.roads.iter() {
            //let mut length = 0;
            distance_so_far.push(total_length as i64);
            let mut segments = vec![];
            for idx in 1..road.len() {
                let a = self.nodes.get(&road[idx - 1]).unwrap();
                let b = self.nodes.get(&road[idx]).unwrap();
                let length = a.distance_mm(b);
                segments.push(length as i64);
                total_length += length;
            }
            road_lengths.push(segments);
            //total_length += length;
        }

        info!("Found a total of {}km of roads", total_length / 1_000_000);

        // Compute 10 million values in [0, total_length]
        let mut rng = rand::thread_rng();
        let nroads = 10_000_000;
        //let nroads = 100;
        let mut offsets: Vec<_> = (1..nroads)
            .map(|_| rng.gen::<i64>().abs() % total_length as i64)
            .collect();
        offsets.sort();
        let offsets = offsets;

        // Find where each offset puts us in the road network
        info!("Calculated {} offsets", offsets.len());

        let bar = ProgressBar::new(offsets.len() as u64);
        bar.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] {bar:40} {pos}/{len} {per_sec} ETA:{eta}"),
        );

        //let mut road_idx = 0;
        //let mut segment_idx = 0;
        //let mut total_distance_so_far = 0;
        //let mut points = vec![];
        let mut road_idx = 0;
        for offset in offsets.iter().progress_with(bar) {
            //let mut road_idx = 0;
            //info!("{}", distance_so_far.len());
            while road_idx + 1 < distance_so_far.len() && distance_so_far[road_idx + 1] < *offset {
                //info!("Incrementing road_idx...");
                road_idx += 1;
            }

            let offset_in_road = offset - distance_so_far[road_idx];
            let mut segment_idx = 0;
            let mut road_length_so_far = 0;
            while segment_idx + 1 < road_lengths[road_idx].len()
                && road_length_so_far + road_lengths[road_idx][segment_idx] < offset_in_road
            {
                road_length_so_far += road_lengths[road_idx][segment_idx];
                segment_idx += 1;
            }
            /*info!(
                "offset={} road_length_so_far={} road_idx={} segment_idx={} distance_so_far={}",
                offset, road_length_so_far, road_idx, segment_idx, distance_so_far[road_idx]
            );*/
            let offset = offset_in_road - road_length_so_far;

            //let offset = offset - total_distance_so_far;
            /*while offset - total_distance_so_far >= road_lengths[road_idx][segment_idx] {
                // Move to the next road
                total_distance_so_far += road_lengths[road_idx][segment_idx];
                //road_idx += 1;
                segment_idx += 1;
                if segment_idx + 1 >= road_lengths[road_idx].len() {
                    segment_idx = 0;
                    if road_idx + 1 < road_lengths.len() {
                        road_idx += 1;
                    }
                }
            }*/
            let a = self.nodes.get(&self.roads[road_idx][segment_idx]).unwrap();
            let b = self
                .nodes
                .get(&self.roads[road_idx][segment_idx + 1])
                .unwrap();
            let alpha = offset as f32 / road_lengths[road_idx][segment_idx] as f32;
            let pnt = a.lerp(alpha, b);
            /*info!("{:?} {:?}", a, b);
            info!(
                "{} {:?} {} {:?}",
                offset, road_lengths[road_idx], alpha, pnt
            );*/
            if alpha < 0.0 || alpha > 1.0 {
                error!("Got invalid alpha value!");
                error!("{:?} {:?}", a, b);
                error!(
                    "{} {:?} {} {:?}",
                    offset, road_lengths[road_idx], alpha, pnt
                );
            } else {
                writer.write(&pnt);
            }
        }
        info!("Finished exporting road points");
    }
}

struct Counter {
    nodes: u64,
    dense_nodes: u64,
    ways: u64,
    relations: u64,
}

impl Counter {
    fn new() -> Counter {
        Counter {
            nodes: 0,
            dense_nodes: 0,
            ways: 0,
            relations: 0,
        }
    }

    fn process(&mut self, element: &Element) {
        match element {
            Element::Way(_) => {
                self.ways += 1;
            }
            Element::Node(_) => {
                self.nodes += 1;
            }
            Element::DenseNode(_) => {
                self.nodes += 1;
                self.dense_nodes += 1;
            }
            Element::Relation(_) => {
                self.relations += 1;
            }
        }
    }
}

fn do_pass<F>(mut cb: F)
where
    F: FnMut(&Element),
{
    let (reader, nnodes) = (
        ElementReader::from_path("/home/lane/Downloads/planet-190812.osm.pbf").unwrap(),
        6461362092u64,
    );
    /*let (reader, nnodes) = (
        ElementReader::from_path("/home/lane/Downloads/texas-latest.osm.pbf").unwrap(),
        57964233u64,
    );*/
    //let (reader, nnodes) = (ElementReader::from_path("/home/lane/Downloads/delaware-latest.osm.pbf").unwrap(), 1703775u64);

    // Increment the counter by one for each way.
    let bar = ProgressBar::new(nnodes);
    bar.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40} {pos}/{len} {per_sec} ETA:{eta}"),
    );
    reader
        .for_each(|element| {
            cb(&element);
            match element {
                Element::Node(_) | Element::DenseNode(_) => {
                    bar.inc(1);
                }
                _ => {}
            }
        })
        .unwrap();
}

#[tokio::main]
async fn main() {
    //let reader = ElementReader::from_path("/home/lane/Downloads/planet-190812.osm.pbf").unwrap();
    //let reader = ElementReader::from_path("/home/lane/Downloads/texas-latest.osm.pbf").unwrap();
    /*let mut ways = 0_u64;
    let mut nodes = 0u64;
    let mut relations = 0u64;*/

    simple_logger::SimpleLogger::new().init().unwrap();

    // TODO: Check that there aren't variances across McDonald's (e.g. capitalization, having wikidata numbers)
    let mut mcdonalds = KvNodeExtractor::new("brand", Some("McDonald's"));
    let mut counter = Counter::new();
    let mut roads = RoadExtractor::new();

    // Increment the counter by one for each way.
    /*let mut bar = ProgressBar::new(57964233u64);
    bar.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40} {pos}/{len} {per_sec} ETA:{eta}"),
    );
    reader
        .for_each(|element| {
            mcdonalds.process(&element);
            counter.process(&element);
            match element {
                Element::Node(_) | Element::DenseNode(_) => {
                    bar.inc(1);
                }
                _ => {}
            }
        })
        .unwrap();*/
    info!(
        "Starting memory usage: {}KB",
        ProcessStats::get().await.unwrap().memory_usage_bytes / 1000
    );

    do_pass(|element| {
        mcdonalds.process(&element);
        counter.process(&element);
        roads.first_pass(&element);
    });

    info!(
        "Number of McDonald's: {} nodes + {} ways",
        mcdonalds.nodes.len(),
        mcdonalds.node_ids.len()
    );
    info!("Number of ways: {}", counter.ways);
    info!(
        "Number of nodes: {} of which {} were dense",
        counter.nodes, counter.dense_nodes
    );
    info!("Number of relations: {}", counter.relations);

    info!(
        "Memory usage after first pass: {}KB",
        ProcessStats::get().await.unwrap().memory_usage_bytes / 1000
    );

    do_pass(|element| {
        mcdonalds.second_pass(&element);
        roads.second_pass(&element);
    });

    info!(
        "Memory usage after second pass: {}KB",
        ProcessStats::get().await.unwrap().memory_usage_bytes / 1000
    );

    mcdonalds.export(FilePointWriter::new("mcdonalds.dat"));
    roads.export(FilePointWriter::new("roads.dat"));
}
