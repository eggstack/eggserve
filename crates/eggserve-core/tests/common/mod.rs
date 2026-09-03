use eggserve_core::primitives::body::BodySource;
use eggserve_core::primitives::canonical::{Response, ResponseBody};
use std::io::{Read, Seek, SeekFrom};

pub fn extract_body_bytes(resp: &Response) -> Vec<u8> {
    match resp.body() {
        Some(ResponseBody::Bytes(bytes)) => bytes.clone(),
        Some(ResponseBody::Empty) | Some(ResponseBody::EmptyWithLength(_)) => Vec::new(),
        Some(ResponseBody::File(source)) => match source {
            BodySource::FileFull { file, len, .. } => {
                let mut bytes = vec![0; *len as usize];
                let mut file = file.try_clone().expect("clone file handle");
                file.read_exact(&mut bytes).expect("read full file");
                bytes
            }
            BodySource::FileRange { file, range, .. } => {
                let mut bytes = vec![0; range.len() as usize];
                let mut file = file.try_clone().expect("clone file handle");
                file.seek(SeekFrom::Start(range.start()))
                    .expect("seek to range start");
                file.read_exact(&mut bytes).expect("read range");
                bytes
            }
            BodySource::Empty => Vec::new(),
            BodySource::Bytes(bytes) => bytes.clone(),
        },
        None => Vec::new(),
    }
}
