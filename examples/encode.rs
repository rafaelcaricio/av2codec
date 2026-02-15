use std::fs;
use std::io::{self, Read};
use std::process;

use wav2c::Encoder;

const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;
const Y_SIZE: usize = (WIDTH * HEIGHT) as usize;
const UV_SIZE: usize = (WIDTH / 2 * HEIGHT / 2) as usize;
const FRAME_SIZE: usize = Y_SIZE + 2 * UV_SIZE;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (output_path, qindex) = parse_args(&args);

    let mut enc = Encoder::new(WIDTH, HEIGHT, qindex).unwrap_or_else(|e| {
        eprintln!("Error creating encoder: {}", e);
        process::exit(1);
    });

    let mut output = enc.headers();
    let mut frame_buf = vec![0u8; FRAME_SIZE];
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

        let y = &frame_buf[..Y_SIZE];
        let u = &frame_buf[Y_SIZE..Y_SIZE + UV_SIZE];
        let v = &frame_buf[Y_SIZE + UV_SIZE..];

        enc.send_frame(y, u, v).unwrap_or_else(|e| {
            eprintln!("Error encoding frame {}: {}", frame_count, e);
            process::exit(1);
        });

        let pkt = enc.receive_packet().expect("expected packet after send_frame");
        eprintln!("Frame {}: {} bytes", pkt.frame_number, pkt.data.len());
        output.extend_from_slice(&pkt.data);
        frame_count += 1;
    }

    enc.flush();

    if frame_count == 0 {
        eprintln!("Error: no frames read from stdin");
        process::exit(1);
    }

    fs::write(&output_path, &output).unwrap_or_else(|e| {
        eprintln!("Error writing output: {}", e);
        process::exit(1);
    });

    eprintln!(
        "Wrote {} frame(s) ({} bytes) to {}",
        frame_count,
        output.len(),
        output_path
    );
}

fn parse_args(args: &[String]) -> (String, u8) {
    if args.len() < 2 {
        eprintln!(
            "Usage: ffmpeg ... -f rawvideo -pix_fmt yuv420p - | encode <output.obu> [--qindex N]"
        );
        eprintln!();
        eprintln!("Example:");
        eprintln!("  ffmpeg -f lavfi -i testsrc2=size=64x64:duration=1:rate=25 \\");
        eprintln!("    -pix_fmt yuv420p -f rawvideo - | cargo run --example encode -- output.obu");
        process::exit(1);
    }

    let output_path = args[1].clone();
    let mut qindex: u8 = 50;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
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

    (output_path, qindex)
}
