use std::fs;

pub struct Y4mHeader {
    pub width: u32,
    pub height: u32,
    pub framerate_num: u32,
    pub framerate_den: u32,
    pub bit_depth: u32,
}

pub struct Y4mFrame {
    pub y: Vec<u16>,
    pub u: Vec<u16>,
    pub v: Vec<u16>,
}

pub struct Y4mReader {
    header: Y4mHeader,
    data: Vec<u8>,
    offset: usize,
}

impl Y4mReader {
    pub fn open(path: &str) -> Result<Self, String> {
        let file_data = fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;

        let newline_pos = file_data
            .iter()
            .position(|&b| b == b'\n')
            .ok_or_else(|| "No newline found in Y4M header".to_string())?;

        let header_line = std::str::from_utf8(&file_data[..newline_pos])
            .map_err(|e| format!("Invalid UTF-8 in header: {}", e))?;

        if !header_line.starts_with("YUV4MPEG2") {
            return Err("Not a valid Y4M file: missing YUV4MPEG2 signature".to_string());
        }

        let mut width: Option<u32> = None;
        let mut height: Option<u32> = None;
        let mut framerate_num: u32 = 30;
        let mut framerate_den: u32 = 1;
        let mut bit_depth: u32 = 8;

        for token in header_line.split(' ').skip(1) {
            if token.is_empty() {
                continue;
            }
            match token.as_bytes()[0] {
                b'W' => {
                    width = Some(
                        token[1..]
                            .parse()
                            .map_err(|_| format!("Invalid width: {}", token))?,
                    );
                }
                b'H' => {
                    height = Some(
                        token[1..]
                            .parse()
                            .map_err(|_| format!("Invalid height: {}", token))?,
                    );
                }
                b'F' => {
                    let parts: Vec<&str> = token[1..].split(':').collect();
                    if parts.len() == 2 {
                        framerate_num = parts[0]
                            .parse()
                            .map_err(|_| format!("Invalid framerate numerator: {}", token))?;
                        framerate_den = parts[1]
                            .parse()
                            .map_err(|_| format!("Invalid framerate denominator: {}", token))?;
                    }
                }
                b'C' => {
                    let cs = &token[1..];
                    if cs == "420p10" {
                        bit_depth = 10;
                    } else if cs == "420" || cs == "420jpeg" || cs == "420mpeg2" || cs == "420paldv"
                    {
                        bit_depth = 8;
                    }
                }
                _ => {}
            }
        }

        let width = width.ok_or_else(|| "Missing width in Y4M header".to_string())?;
        let height = height.ok_or_else(|| "Missing height in Y4M header".to_string())?;

        let header = Y4mHeader {
            width,
            height,
            framerate_num,
            framerate_den,
            bit_depth,
        };

        let remaining_offset = newline_pos + 1;

        Ok(Y4mReader {
            header,
            data: file_data,
            offset: remaining_offset,
        })
    }

    pub fn header(&self) -> &Y4mHeader {
        &self.header
    }

    pub fn next_frame(&mut self) -> Option<Y4mFrame> {
        if self.offset >= self.data.len() {
            return None;
        }

        let remaining = &self.data[self.offset..];

        if !remaining.starts_with(b"FRAME") {
            return None;
        }

        let newline_pos = remaining.iter().position(|&b| b == b'\n')?;
        let frame_data_start = self.offset + newline_pos + 1;

        let w = self.header.width as usize;
        let h = self.header.height as usize;
        let y_size = w * h;
        let uv_width = w / 2;
        let uv_height = h / 2;
        let uv_size = uv_width * uv_height;
        let bytes_per_sample = if self.header.bit_depth > 8 { 2 } else { 1 };
        let y_byte_size = y_size * bytes_per_sample;
        let uv_byte_size = uv_size * bytes_per_sample;
        let total_frame_bytes = y_byte_size + 2 * uv_byte_size;

        if frame_data_start + total_frame_bytes > self.data.len() {
            return None;
        }

        let y_start = frame_data_start;
        let u_start = y_start + y_byte_size;
        let v_start = u_start + uv_byte_size;
        let frame_end = v_start + uv_byte_size;

        let frame = if self.header.bit_depth > 8 {
            Y4mFrame {
                y: self.data[y_start..u_start]
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect(),
                u: self.data[u_start..v_start]
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect(),
                v: self.data[v_start..frame_end]
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect(),
            }
        } else {
            Y4mFrame {
                y: self.data[y_start..u_start]
                    .iter()
                    .map(|&b| b as u16)
                    .collect(),
                u: self.data[u_start..v_start]
                    .iter()
                    .map(|&b| b as u16)
                    .collect(),
                v: self.data[v_start..frame_end]
                    .iter()
                    .map(|&b| b as u16)
                    .collect(),
            }
        };

        self.offset = frame_end;

        Some(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_y4m_header() {
        let reader = Y4mReader::open("tests/fixtures/solid_green_64x64.y4m").unwrap();
        assert_eq!(reader.header().width, 64);
        assert_eq!(reader.header().height, 64);
    }

    #[test]
    fn read_single_frame() {
        let mut reader = Y4mReader::open("tests/fixtures/solid_green_64x64.y4m").unwrap();
        let frame = reader.next_frame().unwrap();
        assert_eq!(frame.y.len(), 64 * 64);
        assert_eq!(frame.u.len(), 32 * 32);
        assert_eq!(frame.v.len(), 32 * 32);
    }

    #[test]
    fn solid_green_pixel_values() {
        let mut reader = Y4mReader::open("tests/fixtures/solid_green_64x64.y4m").unwrap();
        let frame = reader.next_frame().unwrap();
        let y_val = frame.y[0];
        assert!(frame.y.iter().all(|&v| v == y_val));
    }

    #[test]
    fn no_more_frames_after_single() {
        let mut reader = Y4mReader::open("tests/fixtures/solid_green_64x64.y4m").unwrap();
        assert!(reader.next_frame().is_some());
        assert!(reader.next_frame().is_none());
    }
}
