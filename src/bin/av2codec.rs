use std::fs;
use std::process;
use av2codec::encode;
use av2codec::y4m::Y4mReader;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: av2codec <input.y4m> <output.obu> [--qindex N] [--bit-depth 8|10]");
        process::exit(1);
    }

    let input_path = &args[1];
    let output_path = &args[2];

    let mut base_qindex: u8 = 50;
    let mut bit_depth: u32 = 8;
    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--qindex" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --qindex requires a value (0-255)");
                    process::exit(1);
                }
                base_qindex = args[i].parse().unwrap_or_else(|_| {
                    eprintln!("Error: invalid qindex value: {}", args[i]);
                    process::exit(1);
                });
            }
            "--bit-depth" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --bit-depth requires a value (8 or 10)");
                    process::exit(1);
                }
                bit_depth = args[i].parse().unwrap_or_else(|_| {
                    eprintln!("Error: invalid bit-depth value: {}", args[i]);
                    process::exit(1);
                });
                if bit_depth != 8 && bit_depth != 10 {
                    eprintln!("Error: --bit-depth must be 8 or 10, got {}", bit_depth);
                    process::exit(1);
                }
            }
            other => {
                eprintln!("Error: unknown argument: {}", other);
                process::exit(1);
            }
        }
        i += 1;
    }

    let mut reader = Y4mReader::open(input_path).unwrap_or_else(|e| {
        eprintln!("Error: {}", e);
        process::exit(1);
    });

    let width = reader.header().width;
    let height = reader.header().height;
    let input_bit_depth = reader.header().bit_depth;

    if input_bit_depth == 10 && bit_depth == 8 {
        eprintln!(
            "Error: input Y4M is 10-bit but --bit-depth is 8. Use --bit-depth 10 for 10-bit input."
        );
        process::exit(1);
    }

    let mut frames = Vec::new();
    while let Some(frame) = reader.next_frame() {
        if input_bit_depth == 8 && bit_depth == 10 {
            let y: Vec<u16> = frame.y.iter().map(|&v| v << 2).collect();
            let u: Vec<u16> = frame.u.iter().map(|&v| v << 2).collect();
            let v: Vec<u16> = frame.v.iter().map(|&v| v << 2).collect();
            frames.push((y, u, v));
        } else {
            frames.push((frame.y, frame.u, frame.v));
        }
    }

    if frames.is_empty() {
        eprintln!("Error: no frames found in input");
        process::exit(1);
    }

    eprintln!(
        "Encoding {} frame(s) at {}x{} qindex={} bit_depth={}",
        frames.len(),
        width,
        height,
        base_qindex,
        bit_depth
    );

    let bitstream = encode::encode_video_yuv(width, height, base_qindex, &frames, bit_depth);

    fs::write(output_path, &bitstream).unwrap_or_else(|e| {
        eprintln!("Error writing output: {}", e);
        process::exit(1);
    });

    eprintln!("Wrote {} bytes to {}", bitstream.len(), output_path);
}
