//use log::*;
//use rocket::http::{Cookie, Cookies};
//use rocket::request::Form;
//use rocket::request::FromForm;
//use rocket::response::Redirect;
//use rocket::State;
//use rocket_contrib::templates::Template;
use serde_derive::Serialize;
//use std::collections::HashMap;
//use std::sync::Arc;
//use std::sync::Mutex;
//use std::time::{Duration, Instant};

pub type DistanceKm = f64;

fn deg2rad(x: f64) -> f64 {
    3.14159265 * x / 180.0
}

#[derive(Serialize, Clone, Debug)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
}

impl Location {
    pub fn distance_to(&self, other: &Location) -> DistanceKm {
        let lat_diff_sin = ((deg2rad(self.latitude) - deg2rad(other.latitude)) / 2.).sin();
        let lon_diff_sin = ((deg2rad(self.longitude) - deg2rad(other.longitude)) / 2.).sin();
        /*println!(
            "{} {}",
            deg2rad(self.latitude - other.latitude),
            deg2rad(self.longitude - other.longitude)
        );*/
        //println!("{} {}", lat_diff_sin, lon_diff_sin);
        let h = lat_diff_sin * lat_diff_sin
            + deg2rad(self.latitude).cos()
                * deg2rad(other.latitude).cos()
                * lon_diff_sin
                * lon_diff_sin;
        //println!("{}", h);
        let r = 6360.0;
        2.0 * r * h.sqrt().asin()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_distance() {
        let austin = Location {
            latitude: 30.266666,
            longitude: -97.733330,
        };
        let newyork = Location {
            latitude: 40.730610,
            longitude: -73.935242,
        };
        assert_eq!(austin.distance_to(&newyork) as i32, 2432);
    }
}
