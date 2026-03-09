use std::fs;
use std::io::{self, BufWriter, Write};
use std::process;
use av2codec::Decoder;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: av2codec_decode <input.av2> [-o <output.y4m>]");
        process::exit(1);
    }

    let mut input_path: Option<&str> = None;
    let mut output_path: Option<&str> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: {} requires a value", args[i - 1]);
                    process::exit(1);
                }
                output_path = Some(&args[i]);
            }
            arg if arg.starts_with('-') => {
                eprintln!("Error: unknown argument: {}", arg);
                process::exit(1);
            }
            _ => {
                if input_path.is_some() {
                    eprintln!("Error: unexpected positional argument: {}", args[i]);
                    process::exit(1);
                }
                input_path = Some(&args[i]);
            }
        }
        i += 1;
    }

    let input_path = input_path.unwrap_or_else(|| {
        eprintln!("Error: missing input file path");
        process::exit(1);
    });

    let data = fs::read(input_path).unwrap_or_else(|e| {
        eprintln!("Error reading {}: {}", input_path, e);
        process::exit(1);
    });

    let frames = Decoder::decode_all(&data).unwrap_or_else(|e| {
        eprintln!("Error decoding bitstream: {}", e);
        process::exit(1);
    });

    if frames.is_empty() {
        eprintln!("Error: no frames decoded");
        process::exit(1);
    }

    let width = frames[0].width;
    let height = frames[0].height;

    let writer: BufWriter<Box<dyn Write>> = match output_path {
        Some(path) => {
            let file = fs::File::create(path).unwrap_or_else(|e| {
                eprintln!("Error creating {}: {}", path, e);
                process::exit(1);
            });
            BufWriter::new(Box::new(file))
        }
        None => BufWriter::new(Box::new(io::stdout().lock())),
    };

    write_y4m(writer, width, height, &frames).unwrap_or_else(|e| {
        eprintln!("Error writing Y4M: {}", e);
        process::exit(1);
    });

    eprintln!("Decoded {} frame(s) at {}x{}", frames.len(), width, height);
}

fn write_y4m(
    mut w: impl Write,
    width: u32,
    height: u32,
    frames: &[av2codec::DecodedFrame],
) -> io::Result<()> {
    writeln!(w, "YUV4MPEG2 W{} H{} F30:1 Ip A1:1 C420", width, height)?;

    let uv_width = (width / 2) as usize;
    let uv_height = (height / 2) as usize;

    for frame in frames {
        w.write_all(b"FRAME\n")?;

        for row in 0..height as usize {
            w.write_all(&frame.y[row * width as usize..(row + 1) * width as usize])?;
        }

        for row in 0..uv_height {
            w.write_all(&frame.u[row * uv_width..(row + 1) * uv_width])?;
        }

        for row in 0..uv_height {
            w.write_all(&frame.v[row * uv_width..(row + 1) * uv_width])?;
        }
    }

    w.flush()?;
    Ok(())
}
