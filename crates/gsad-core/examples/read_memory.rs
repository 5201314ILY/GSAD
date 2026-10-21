use std::sync::Arc;

use gsad_core::{source::MemorySource, DataSource};

fn main() -> gsad_core::Result<()> {
    let source: Arc<dyn DataSource> = Arc::new(MemorySource::with_label(
        "demo.txt",
        Arc::<[u8]>::from(&b"GSAD core source contract\n"[..]),
    ));

    let info = source.info();
    println!("source: {} (length: {:?})", info.label, info.known_length);
    let mut reader = source.open_reader()?;
    let mut buffer = [0_u8; 8];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        print!("{}", String::from_utf8_lossy(&buffer[..count]));
    }
    Ok(())
}
