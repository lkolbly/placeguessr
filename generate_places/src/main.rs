use indicatif::{ProgressBar, ProgressIterator, ProgressStyle};
use log::*;
use osmpbf::{Element, ElementReader};
use rand::prelude::*;
use simple_process_stats::ProcessStats;
use std::collections::{HashMap, HashSet};
use std::io::prelude::*;

#[derive(PartialEq, Debug, Clone)]
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

    /// This is an optimization, so the output can be used many times
    points: Vec<Location>,
}

impl RoadExtractor {
    fn new() -> RoadExtractor {
        RoadExtractor {
            node_ids: HashSet::new(),
            nodes: HashMap::new(),
            roads: vec![],
            points: vec![],
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

    fn compute_points(&mut self) {
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
                //writer.write(&pnt);
                self.points.push(pnt);
            }
        }
        info!("Finished exporting road points");
    }

    fn export(&self, mut writer: impl PointWriter) {
        for point in self.points.iter() {
            writer.write(&point);
        }
    }
}

struct DebugPointTee<T: PointWriter> {
    writer: T,
    file: std::fs::File,
}

impl<T: PointWriter> DebugPointTee<T> {
    fn new(path: &str, writer: T) -> DebugPointTee<T> {
        DebugPointTee {
            writer: writer,
            file: std::fs::File::create(path).unwrap(),
        }
    }
}

impl<T: PointWriter> PointWriter for DebugPointTee<T> {
    fn write(&mut self, location: &Location) {
        self.writer.write(location);
        self.file
            .write_all(format!("{},{}\n", location.latitude, location.longitude,).as_bytes())
            .unwrap();
    }
}

struct BoundaryFilter {
    edges: Vec<(Location, Location)>,
}

fn lerp(x1: f32, x2: f32, y1: f32, y2: f32, x: f32) -> f32 {
    let a = (x - x1) / (x2 - x1);
    (y2 - y1) * a + y1
}

impl BoundaryFilter {
    fn contains(&self, location: &Location) -> bool {
        let mut num_crossings = 0;
        for (a, b) in self.edges.iter() {
            if a.longitude <= location.longitude && b.longitude <= location.longitude {
                // Line is too far West to matter
            } else if a.longitude >= location.longitude && b.longitude >= location.longitude {
                // Line is too far East to matter
            } else if a.latitude >= location.latitude && b.latitude >= location.latitude {
                // Line is too far North to matter
            } else if a.latitude <= location.latitude && b.latitude <= location.latitude {
                num_crossings += 1;
            } else {
                let latitude = lerp(
                    a.longitude,
                    b.longitude,
                    a.latitude,
                    b.latitude,
                    location.longitude,
                );
                if latitude <= location.latitude {
                    num_crossings += 1;
                }
            }
        }
        num_crossings % 2 == 1
    }
}

struct BoundaryFilterWriter<T: PointWriter> {
    writer: T,

    /// This should be sorted by longitude of the first element
    //edges: Vec<(Location, Location)>,
    filters: Vec<BoundaryFilter>,
}

impl<T: PointWriter> BoundaryFilterWriter<T> {
    fn new(filters: Vec<BoundaryFilter>, writer: T) -> BoundaryFilterWriter<T> {
        BoundaryFilterWriter { writer, filters }
    }
}

impl<T: PointWriter> PointWriter for BoundaryFilterWriter<T> {
    fn write(&mut self, location: &Location) {
        /*let mut num_crossings = 0;
        for (a, b) in self.edges.iter() {
            if a.longitude <= location.longitude && b.longitude <= location.longitude {
                // Line is too far West to matter
            } else if a.longitude >= location.longitude && b.longitude >= location.longitude {
                // Line is too far East to matter
            } else if a.latitude >= location.latitude && b.latitude >= location.latitude {
                // Line is too far North to matter
            } else if a.latitude <= location.latitude && b.latitude <= location.latitude {
                num_crossings += 1;
            } else {
                let latitude = lerp(
                    a.longitude,
                    b.longitude,
                    a.latitude,
                    b.latitude,
                    location.longitude,
                );
                if latitude <= location.latitude {
                    num_crossings += 1;
                }
            }
        }
        if num_crossings % 2 == 1 {*/
        if self
            .filters
            .iter()
            .fold(false, |acc, x| acc | x.contains(location))
        {
            self.writer.write(location);
        }
    }
}

type RelId = i64;
type WayId = i64;
type NodeId = i64;

struct BoundaryFinder {
    boundaries: HashMap<String, RelId>,
    boundary_ways: HashMap<RelId, Vec<WayId>>,
    way_ids: HashSet<WayId>,
    ways: HashMap<WayId, Vec<NodeId>>,
    node_ids: HashSet<NodeId>,
    nodes: HashMap<NodeId, Location>,
}

impl BoundaryFinder {
    fn new() -> BoundaryFinder {
        BoundaryFinder {
            boundaries: HashMap::new(),
            boundary_ways: HashMap::new(),
            way_ids: HashSet::new(),
            ways: HashMap::new(),
            node_ids: HashSet::new(),
            nodes: HashMap::new(),
        }
    }

    fn dump_to_file(&self, relid: i64, filepath: &str) {
        for (k, ways) in self.boundary_ways.iter() {
            info!("Boundary {} has {} ways", k, ways.len());
        }

        let mut edges = vec![];
        for way_id in self.boundary_ways.get(&relid).unwrap().iter() {
            for nodes in self.ways.get(way_id).unwrap().windows(2) {
                let node_a = self.nodes.get(&nodes[0]).unwrap();
                let node_b = self.nodes.get(&nodes[1]).unwrap();
                edges.push(((*node_a).clone(), (*node_b).clone()));
            }
        }
        let mut file = std::fs::File::create("geo.csv").unwrap();
        for (a, b) in edges.iter() {
            file.write_all(
                format!(
                    "{},{},{},{}\n",
                    a.latitude, a.longitude, b.latitude, b.longitude
                )
                .as_bytes(),
            )
            .unwrap();
        }
    }

    fn filter(&self, relid: i64) -> BoundaryFilter {
        if !self.boundary_ways.contains_key(&relid) {
            error!("Could not find boundary relation ID {}!", relid);
            return BoundaryFilter { edges: vec![] };
        }
        let mut edges = vec![];
        for way_id in self.boundary_ways.get(&relid).unwrap().iter() {
            if !self.ways.contains_key(way_id) {
                error!(
                    "Could not find way {} inside boundary relation {}",
                    way_id, relid
                );
                return BoundaryFilter { edges: vec![] };
            }
            for nodes in self.ways.get(way_id).unwrap().windows(2) {
                let node_a = self.nodes.get(&nodes[0]).unwrap();
                let node_b = self.nodes.get(&nodes[1]).unwrap();
                edges.push(((*node_a).clone(), (*node_b).clone()));
            }
        }
        edges.sort_by(|(a, _), (b, _)| a.longitude.partial_cmp(&b.longitude).unwrap());
        BoundaryFilter { edges }
    }

    /// Find all "ways" which are a part of a border
    fn find_boundaries(&mut self, element: &Element) {
        match element {
            Element::DenseNode(_) | Element::Node(_) => {}
            Element::Way(way) => {
                /*for (k, v) in way.tags() {
                    if k == "boundary" && v == "administrative" {
                        self.boundary_ways.insert(way.id());
                    }
                }*/
            }
            Element::Relation(rel) => {
                let mut name = String::new();
                for (k, v) in rel.tags() {
                    if k == "name" {
                        name = v.to_string();
                        break;
                    }
                }
                for (k, v) in rel.tags() {
                    if k == "boundary" && v == "administrative" {
                        let mut way_ids = vec![];
                        for member in rel.members() {
                            match member.role() {
                                Err(_) => {
                                    panic!(format!(
                                        "Found unknown member role {}!",
                                        member.role_sid
                                    ));
                                }
                                Ok("outer") | Ok("inner") => {
                                    if member.member_type
                                        == osmpbf::elements::RelMemberType::Relation
                                    {
                                        panic!("Found relation as outer/inner member of relation! Where is this allowed?");
                                    }
                                    //self.boundary_ways.push(member.member_id);
                                    way_ids.push(member.member_id);
                                    self.way_ids.insert(member.member_id);
                                }
                                Ok(_) => {
                                    // Ignore
                                }
                            }
                        }
                        self.boundaries.insert(name, rel.id());
                        self.boundary_ways.insert(rel.id(), way_ids);
                        break;
                    }
                }
            }
        }
    }

    /// Enumerate all nodes which are a part of a border way
    fn find_node_ids(&mut self, element: &Element) {
        match element {
            Element::DenseNode(node) => {
                //
            }
            Element::Node(node) => {
                //
            }
            Element::Way(way) => {
                if !self.way_ids.contains(&way.id()) {
                    return;
                }
                // Check that this way has the boundary tag
                /*let mut has_boundary_tag = false;
                for (k, v) in way.tags() {
                    if k == "boundary" && v == "administrative" {
                        has_boundary_tag = true;
                        break;
                    }
                }
                if !has_boundary_tag {
                    log::warn!("Found way {} without boundary tag!", way.id());
                }*/

                // Extract all the node IDs
                let node_ids: Vec<i64> = way.refs().collect();
                for node_id in node_ids.iter() {
                    self.node_ids.insert(*node_id);
                }
                self.ways.insert(way.id(), node_ids);
            }
            Element::Relation(rel) => {
                // Ignore
            }
        }
    }

    /// Extract the lat/lng of every border node
    fn find_nodes(&mut self, element: &Element) {
        match element {
            Element::DenseNode(node) => {
                if self.node_ids.contains(&node.id) {
                    self.nodes
                        .insert(node.id, Location::new(node.lat(), node.lon()));
                }
            }
            Element::Node(node) => {
                if self.node_ids.contains(&node.id()) {
                    self.nodes
                        .insert(node.id(), Location::new(node.lat(), node.lon()));
                }
            }
            Element::Way(way) => {
                // Ignore
            }
            Element::Relation(rel) => {
                // Ignore
            }
        }
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

fn do_pass<F>(/*path: &str, nnodes: u64,*/ mut cb: F)
where
    F: FnMut(&Element),
{
    //let reader = ElementReader::from_path(path).unwrap();
    let (reader, nnodes) = (
        ElementReader::from_path("/home/lane/Downloads/planet-190812.osm.pbf").unwrap(),
        6461362092u64,
    );
    /*let (reader, nnodes) = (
        ElementReader::from_path("/home/lane/Downloads/texas-latest.osm.pbf").unwrap(),
        57964233u64,
    );*/
    /*let (reader, nnodes) = (
        ElementReader::from_path("/home/lane/Downloads/delaware-latest.osm.pbf").unwrap(),
        1703775u64,
    );*/

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
    let mut walmart = KvNodeExtractor::new("brand", Some("Walmart"));
    let mut counter = Counter::new();
    let mut roads = RoadExtractor::new();
    let mut geographic_filter = BoundaryFinder::new();

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
        walmart.process(&element);
        counter.process(&element);
        roads.first_pass(&element);
        geographic_filter.find_boundaries(&element);
    });

    info!(
        "Number of McDonald's: {} nodes + {} ways",
        mcdonalds.nodes.len(),
        mcdonalds.node_ids.len()
    );
    info!(
        "Number of Walmarts: {} nodes + {} ways",
        walmart.nodes.len(),
        walmart.node_ids.len()
    );
    info!("Number of ways: {}", counter.ways);
    info!(
        "Number of nodes: {} of which {} were dense",
        counter.nodes, counter.dense_nodes
    );
    info!("Number of relations: {}", counter.relations);
    info!(
        "Number of boundary relations: {}",
        geographic_filter.boundary_ways.len()
    );

    info!(
        "Memory usage after first pass: {}KB",
        ProcessStats::get().await.unwrap().memory_usage_bytes / 1000
    );

    do_pass(|element| {
        mcdonalds.second_pass(&element);
        walmart.second_pass(&element);
        roads.second_pass(&element);
        geographic_filter.find_node_ids(&element);
    });

    info!(
        "Memory usage after second pass: {}KB",
        ProcessStats::get().await.unwrap().memory_usage_bytes / 1000
    );

    do_pass(|element| {
        geographic_filter.find_nodes(&element);
    });

    info!(
        "Memory usage after third pass: {}KB",
        ProcessStats::get().await.unwrap().memory_usage_bytes / 1000
    );

    info!(
        "Boundary relations/ways/nodes: {}/{}/{}",
        geographic_filter.boundaries.len(),
        geographic_filter.way_ids.len(),
        geographic_filter.node_ids.len()
    );

    roads.compute_points();

    mcdonalds.export(FilePointWriter::new("mcdonalds.dat"));
    walmart.export(FilePointWriter::new("walmart.dat"));
    roads.export(FilePointWriter::new("roads.dat"));
    geographic_filter.dump_to_file(117177, "tmp.csv");
    /*roads.export(geographic_filter.filter(
        117177,
        DebugPointTee::new("pnts.csv", FilePointWriter::new("roads-cheswold.dat")),
    ));*/
    let g = geographic_filter;
    roads.export(BoundaryFilterWriter::new(
        vec![g.filter(117177)],
        FilePointWriter::new("roads-cheswold.dat"),
    ));
    roads.export(BoundaryFilterWriter::new(
        vec![g.filter(114690)],
        FilePointWriter::new("roads-texas.dat"),
    ));
    roads.export(BoundaryFilterWriter::new(
        vec![g.filter(148838)],
        FilePointWriter::new("roads-us.dat"),
    ));
    roads.export(BoundaryFilterWriter::new(
        vec![
            g.filter(16239),   // Austria
            g.filter(52411),   // Belgium
            g.filter(214885),  // Croatia
            g.filter(307787),  // Cyprus
            g.filter(51684),   // Czechia
            g.filter(50046),   // Denmark
            g.filter(79510),   // Estonia
            g.filter(54224),   // Finland
            g.filter(2202162), // France
            g.filter(51477),   // Germany
            g.filter(192307),  // Greece
            g.filter(62273),   // Ireland
            g.filter(365331),  // Italy
            g.filter(72594),   // Latvia
            g.filter(72596),   // Lithuania
            g.filter(2171347), // Luxembourg
            g.filter(365307),  // Malta
            g.filter(47796),   // Netherlands
            g.filter(21335),   // Hungary
            g.filter(49715),   // Poland
            g.filter(295480),  // Portugal
            g.filter(1311341), // Spain
            g.filter(218657),  // Slovenia
            g.filter(14296),   // Slovakia
            g.filter(52822),   // Sweden
            g.filter(90689),   // Romania
        ],
        FilePointWriter::new("roads-eu.dat"),
    ));
}
