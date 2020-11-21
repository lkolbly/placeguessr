use rand::Rng;
use std::collections::HashMap;
use std::convert::TryInto;

use crate::location::Location;

pub trait LocationGeneratorTrait {
    fn sample_from_dataset(&self, dataset: &str) -> Location;
}

pub struct DatafileLocationGenerator {
    //points: Vec<Location>,
    datasets: HashMap<String, Vec<Location>>,
}

//unsafe impl Send for DatafileLocationGenerator {}
//unsafe impl Sync for DatafileLocationGenerator {}

fn load_file(filename: &str) -> Vec<Location> {
    let contents = std::fs::read(filename).unwrap();
    let mut idx = 0;
    let mut points = vec![];
    while idx < contents.len() {
        let lat = f32::from_le_bytes(contents[idx..idx + 4].try_into().unwrap());
        let lon = f32::from_le_bytes(contents[idx + 4..idx + 8].try_into().unwrap());
        idx += 8;
        points.push(Location {
            latitude: lat as f64,
            longitude: lon as f64,
        });
    }
    points
}

impl DatafileLocationGenerator {
    pub fn new(datasets: &[(&str, &str)]) -> DatafileLocationGenerator {
        let mut gen = DatafileLocationGenerator {
            datasets: HashMap::new(),
        };
        for (name, filename) in datasets.iter() {
            gen.datasets.insert(name.to_string(), load_file(filename));
        }
        gen
    }
}

impl LocationGeneratorTrait for DatafileLocationGenerator {
    fn sample_from_dataset(&self, dataset: &str) -> Location {
        let mut rng = rand::thread_rng();
        let idx: usize = rng.gen();
        match self.datasets.get(dataset) {
            Some(points) => points[idx % points.len()].clone(),
            None => {
                let points = self.datasets.get("world").unwrap();
                points[idx % points.len()].clone()
            }
        }
    }
}

pub struct MockLocationGenerator {}

impl MockLocationGenerator {
    pub fn new() -> MockLocationGenerator {
        MockLocationGenerator {}
    }
}

impl LocationGeneratorTrait for MockLocationGenerator {
    fn sample_from_dataset(&self, _dataset: &str) -> Location {
        Location {
            latitude: 30.0,
            longitude: 98.0,
        }
    }
}

pub enum LocationGenerator {
    Datafile(DatafileLocationGenerator),
    Mock(MockLocationGenerator),
}

impl LocationGenerator {
    pub fn mock() -> LocationGenerator {
        LocationGenerator::Mock(MockLocationGenerator::new())
    }

    pub fn from_datafile(datasets: &[(&str, &str)]) -> LocationGenerator {
        LocationGenerator::Datafile(DatafileLocationGenerator::new(datasets))
    }
}

impl LocationGeneratorTrait for LocationGenerator {
    fn sample_from_dataset(&self, dataset: &str) -> Location {
        match self {
            LocationGenerator::Datafile(x) => x.sample_from_dataset(dataset),
            LocationGenerator::Mock(x) => x.sample_from_dataset(dataset),
        }
    }
}

/*pub fn generate_location() -> Location {
    let mut rng = rand::thread_rng();
    Location {
        latitude: rng.gen::<f64>() + 30.,
        longitude: rng.gen::<f64>() - 98.,
    }
}*/
