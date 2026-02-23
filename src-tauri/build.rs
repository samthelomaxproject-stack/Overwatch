use std::io::Result;

fn main() -> Result<()> {
    // Compile Meshtastic protobuf definitions
    prost_build::compile_protos(
        &["proto/meshtastic.proto"],
        &["proto/"],
    )?;
    
    println!("cargo:rerun-if-changed=proto/meshtastic.proto");
    
    tauri_build::build();
    
    Ok(())
}