use std::fs;
use std::io::{self, Read};
use std::process;

use av2codec::Encoder;

struct Config {
    output_path: String,
    width: u32,
    height: u32,
    qindex: u8,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let config = parse_args(&args);

    let y_size = (config.width * config.height) as usize;
    let uv_size = (config.width / 2 * config.height / 2) as usize;
    let frame_size = y_size + 2 * uv_size;

    let mut enc = Encoder::new(config.width, config.height, config.qindex, 8).unwrap_or_else(|e| {
        eprintln!("Error creating encoder: {}", e);
        process::exit(1);
    });

    let mut output = enc.headers();
    let mut frame_buf = vec![0u8; frame_size];
    let mut frame_count = 0u64;
    let stdin = io::stdin();
    let mut reader = stdin.lock();

    loop {
        match reader.read_exact(&mut frame_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => {
                eprintln!("Error reading stdin: {}", e);
                process::exit(1);
            }
        }

        let y: Vec<u16> = frame_buf[..y_size].iter().map(|&b| b as u16).collect();
        let u: Vec<u16> = frame_buf[y_size..y_size + uv_size].iter().map(|&b| b as u16).collect();
        let v: Vec<u16> = frame_buf[y_size + uv_size..].iter().map(|&b| b as u16).collect();

        enc.send_frame(&y, &u, &v).unwrap_or_else(|e| {
            eprintln!("Error encoding frame {}: {}", frame_count, e);
            process::exit(1);
        });

        let pkt = enc
            .receive_packet()
            .expect("expected packet after send_frame");
        eprintln!("Frame {}: {} bytes", pkt.frame_number, pkt.data.len());
        output.extend_from_slice(&pkt.data);
        frame_count += 1;
    }

    enc.flush();

    if frame_count == 0 {
        eprintln!("Error: no frames read from stdin");
        process::exit(1);
    }

    fs::write(&config.output_path, &output).unwrap_or_else(|e| {
        eprintln!("Error writing output: {}", e);
        process::exit(1);
    });

    eprintln!(
        "Wrote {} frame(s) ({}x{}, {} bytes) to {}",
        frame_count,
        config.width,
        config.height,
        output.len(),
        config.output_path
    );
}

fn parse_args(args: &[String]) -> Config {
    if args.len() < 2 {
        eprintln!(
            "Usage: ffmpeg ... -f rawvideo -pix_fmt yuv420p - | encode <output.obu> [options]"
        );
        eprintln!();
        eprintln!("Options:");
        eprintln!("  --width N    Frame width (default: 64, must be multiple of 64)");
        eprintln!("  --height N   Frame height (default: 64, must be multiple of 64)");
        eprintln!("  --qindex N   Quantization index 0-255 (default: 50)");
        eprintln!();
        eprintln!("Example:");
        eprintln!("  ffmpeg -f lavfi -i testsrc2=size=128x128:duration=1:rate=25 \\");
        eprintln!(
            "    -pix_fmt yuv420p -f rawvideo - | cargo run --example encode -- output.obu --width 128 --height 128"
        );
        process::exit(1);
    }

    let output_path = args[1].clone();
    let mut width: u32 = 64;
    let mut height: u32 = 64;
    let mut qindex: u8 = 50;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--width" => {
                i += 1;
                width = args.get(i).and_then(|s| s.parse().ok()).unwrap_or_else(|| {
                    eprintln!("Error: --width requires a value");
                    process::exit(1);
                });
            }
            "--height" => {
                i += 1;
                height = args.get(i).and_then(|s| s.parse().ok()).unwrap_or_else(|| {
                    eprintln!("Error: --height requires a value");
                    process::exit(1);
                });
            }
            "--qindex" => {
                i += 1;
                qindex = args.get(i).and_then(|s| s.parse().ok()).unwrap_or_else(|| {
                    eprintln!("Error: --qindex requires a value (0-255)");
                    process::exit(1);
                });
            }
            other => {
                eprintln!("Error: unknown argument: {}", other);
                process::exit(1);
            }
        }
        i += 1;
    }

    Config {
        output_path,
        width,
        height,
        qindex,
    }
}
