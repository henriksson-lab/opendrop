# Tiselius

An open-source, cross-platform GUI to control UV-Vis spectrophotometers

**Hardware support:**

* **NanoDrop ND-1000** microvolume UV-Vis spectrophotometer (Not affiliated with or endorsed by Thermo Fisher Scientific)


**Still being developed**

## Screenshot

A single window: a measurement toolbar, an overlaid spectrum plot, and a table
of acquired samples. Select rows to overlay their spectra; export the plot and
table to PDF from the File menu. Feature requests are welcome (use github issues).

![Tiselius](docs/images/tiselius.png)

## Building & running

```sh
cargo run          # launch the GUI (mock backend)
cargo test         # run all tests
cargo clippy       # lint
```

### Using Tiselius as a library

Disable default features to depend on the measurement/device/format code
without pulling in Slint:

```toml
[dependencies]
tiselius = { version = "0.1", default-features = false }
```

```rust
use tiselius::measure::{calc, Spectrum};
use tiselius::formats::read_archive;
```

## License

MIT license

Note that the code has been generated using agentic AI. Thus, before using reusing parts, please check
if the code has been directly copied from somewhere else
