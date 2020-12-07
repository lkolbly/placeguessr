PlaceGuessr
===========

This is PlaceGuessr! The definitely-not-a-clone-of-GeoGuessr I made.

In order to run your own instance, you will need an API key from the Google Cloud Console which allows access to the Maps JavaScript API. Create a file, `config.yaml`, with contents copied from `config.yaml.example` with your google API key substituted.

Then, you will need to run the generate_places executable to generate points. Obtain (preferably via BitTorrent) a copy of the OpenStreetMap [planet.osm.pbf](https://wiki.openstreetmap.org/wiki/Planet.osm) file. Set the path and estimated number of nodes (to get an accurate progress bar) in `generate_places/src/main.rs:do_pass()`, then run it to generate the `.dat` files.

Once you have those, copy them to the working directory, and run the main program! (you will need nightly because this project uses Rocket)
```
$ cargo +nightly run --release
```

TODO:
=====
* Remove hardcoding of what regions are available: In generator, read region specifier from some sort of config file.
  * Config file needs to support all the current place generators (filter by region, filter by OSM tag/value)
* Multiplayer.
* Allow configurable URL roots (currently is fixed at `/placeguessr`)
