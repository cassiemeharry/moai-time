# moai-time

This is a simple command line utility to estimate print times for Peopoly Moai
gcode files.

## Usage

Run the program with `cargo run --`, passing the filenames of the gcode files
you wish to analyze. The utility will process the file and output the print time
estimate.

```bash
$ cargo run --release -- path/to/sliced.gcode
For path/to/sliced.gcode:
    Estimated print time: 2 hours and 18 minutes
               Laser: 29 minutes and 36 seconds
        Layer change: 1 hour and 49 minutes
```
