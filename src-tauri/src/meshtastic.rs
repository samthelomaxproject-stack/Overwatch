use crate::*;
use crate::meshtastic_proto::*;
use prost::Message;
use std::io::{Read, Write};
use std::time::{Duration, Instant};
use bytes::BytesMut;
use serde::Serialize;

// Constants
const START_BYTE: u8 = 0x94;
const HEADER_LEN: usize = 4;
const MAX_PACKET_SIZE: usize = 512;

#[derive(Clone, Debug, Serialize)]
pub struct MeshtasticNode {
    pub node_id: u32,
    pub long_name: String,
    pub short_name: String,
    #[serde(skip)]
    pub last_seen: Option<Instant>,
    pub snr: f32,
    pub hops_away: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct MeshtasticMessage {
    pub from: u32,
    pub to: u32,
    pub text: String,
    pub timestamp: u64,
    pub channel: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct MeshtasticPosition {
    pub node_id: u32,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: i32,
    pub timestamp: u32,
    pub precision: u32,
}

pub struct MeshtasticState {
    pub is_connected: AtomicBool,
    pub nodes: Arc<Mutex<Vec<MeshtasticNode>>>,
    pub messages: Arc<Mutex<Vec<MeshtasticMessage>>>,
    pub positions: Arc<Mutex<Vec<MeshtasticPosition>>>,
    pub my_node_id: Arc<Mutex<Option<u32>>>,
}

impl Clone for MeshtasticState {
    fn clone(&self) -> Self {
        Self {
            is_connected: AtomicBool::new(self.is_connected.load(Ordering::SeqCst)),
            nodes: self.nodes.clone(),
            messages: self.messages.clone(),
            positions: self.positions.clone(),
            my_node_id: self.my_node_id.clone(),
        }
    }
}

impl MeshtasticState {
    pub fn new() -> Self {
        Self {
            is_connected: AtomicBool::new(false),
            nodes: Arc::new(Mutex::new(Vec::new())),
            messages: Arc::new(Mutex::new(Vec::new())),
            positions: Arc::new(Mutex::new(Vec::new())),
            my_node_id: Arc::new(Mutex::new(None)),
        }
    }
}

pub fn start_meshtastic_serial(app_handle: tauri::AppHandle, state: Arc<MeshtasticState>, port: String) -> Result<String, String> {
    if state.is_connected.load(Ordering::SeqCst) {
        return Err("Meshtastic already connected".to_string());
    }
    
    state.is_connected.store(true, Ordering::SeqCst);
    let state_clone = state.clone();
    let port_clone = port.clone();
    
    thread::spawn(move || {
        eprintln!("Connecting to Meshtastic on port: {}", port_clone);
        let _ = app_handle.emit("meshtastic-debug", "Thread started, entering baud rate loop");
        
        // Try 921600 first (newer firmware), fall back to 115200 (older)
        let baud_rates = [921600, 115200];
        let mut port_result = None;
        
        for &baud in &baud_rates {
            let msg = format!("Trying baud rate: {}", baud);
            eprintln!("{}", msg);
            let _ = app_handle.emit("meshtastic-debug", msg);
            match serialport::new(&port_clone, baud)
                .timeout(Duration::from_millis(100))
                .data_bits(serialport::DataBits::Eight)
                .parity(serialport::Parity::None)
                .stop_bits(serialport::StopBits::One)
                .flow_control(serialport::FlowControl::None)
                .open() {
                Ok(mut p) => {
                    eprintln!("Opened port at {} baud", baud);
                    let _ = app_handle.emit("meshtastic-debug", format!("Port opened at {} baud", baud));
                    // Set DTR and RTS - some devices need this to enable TX
                    if let Err(e) = p.write_data_terminal_ready(true) {
                        let _ = app_handle.emit("meshtastic-debug", format!("Warning: Could not set DTR: {}", e));
                    }
                    if let Err(e) = p.write_request_to_send(true) {
                        let _ = app_handle.emit("meshtastic-debug", format!("Warning: Could not set RTS: {}", e));
                    }
                    let _ = app_handle.emit("meshtastic-debug", "DTR/RTS set");
                    port_result = Some(p);
                    break;
                }
                Err(e) => {
                    let err_msg = format!("Failed to open at {} baud: {}", baud, e);
                    eprintln!("{}", err_msg);
                    let _ = app_handle.emit("meshtastic-debug", err_msg);
                }
            }
        }
        
        match port_result {
            Some(mut port) => {
                eprintln!("Serial port opened successfully");
                let _ = app_handle.emit("meshtastic-status", "connected");
                
                let mut buffer = BytesMut::with_capacity(MAX_PACKET_SIZE);
                let mut packet_buffer = Vec::new();
                let mut in_packet = false;
                let mut packet_len: usize = 0;
                let mut bytes_read = 0;
                
                // Request config
                use meshtastic_proto::to_radio::PayloadVariant;
                let config_request = ToRadio {
                    payload_variant: Some(PayloadVariant::WantConfigId(0)),
                };
                match send_protobuf(&mut port, config_request) {
                    Ok(_) => {
                        let _ = app_handle.emit("meshtastic-debug", "Config request sent successfully");
                    }
                    Err(e) => {
                        let _ = app_handle.emit("meshtastic-debug", format!("Failed to send config request: {}", e));
                    }
                }
                
                let mut last_data_time = Instant::now();
                let mut packets_received = 0u32;
                let mut last_heartbeat = Instant::now();
                
                loop {
                    if !state_clone.is_connected.load(Ordering::SeqCst) {
                        break;
                    }
                    
                    // Heartbeat every 5 seconds
                    if last_heartbeat.elapsed() > Duration::from_secs(5) {
                        let _ = app_handle.emit("meshtastic-debug", format!("Heartbeat: connected, {} packets received", packets_received));
                        last_heartbeat = Instant::now();
                    }
                    
                    let mut byte = [0u8; 1];
                    match port.read(&mut byte) {
                        Ok(n) => {
                            if n == 0 { continue; }
                            let b = byte[0];
                            last_data_time = Instant::now(); // We got data!
                            
                            // Debug: log START_BYTE
                            if b == START_BYTE {
                                let _ = app_handle.emit("meshtastic-debug", "START_BYTE received");
                            }
                            
                            if !in_packet {
                                if b == START_BYTE {
                                    in_packet = true;
                                    packet_buffer.clear();
                                    bytes_read = 0;
                                    packet_len = 0;
                                }
                            } else {
                                if bytes_read == 0 {
                                    // MSB of length
                                    packet_len = (b as usize) << 8;
                                } else if bytes_read == 1 {
                                    // LSB of length
                                    packet_len |= b as usize;
                                    if packet_len > MAX_PACKET_SIZE {
                                        eprintln!("Packet too large: {}", packet_len);
                                        in_packet = false;
                                        continue;
                                    }
                                } else if bytes_read < HEADER_LEN - 1 {
                                    // Header byte (usually 0x00)
                                } else {
                                    // Payload
                                    packet_buffer.push(b);
                                    if packet_buffer.len() >= packet_len {
                                        // Parse packet
                                        eprintln!("Attempting to decode packet of {} bytes", packet_len);
                                        let _ = app_handle.emit("meshtastic-debug", format!("Decoding {} byte packet", packet_len));
                                        
                                        match FromRadio::decode(&packet_buffer[..]) {
                                            Ok(from_radio) => {
                                                eprintln!("Packet decoded successfully!");
                                                let _ = app_handle.emit("meshtastic-debug", "Packet decoded OK");
                                                handle_from_radio(&app_handle, &state_clone, from_radio);
                                            }
                                            Err(e) => {
                                                eprintln!("Failed to decode packet: {}", e);
                                                let _ = app_handle.emit("meshtastic-debug", format!("Decode error: {}", e));
                                            }
                                        }
                                        in_packet = false;
                                    }
                                }
                                bytes_read += 1;
                            }
                        }
                        Ok(_) => {}
                        Err(e) => {
                            if e.kind() != std::io::ErrorKind::TimedOut {
                                eprintln!("Serial read error: {}", e);
                                let _ = app_handle.emit("meshtastic-error", format!("Read error: {}", e));
                                break;
                            }
                            
                            // Check for no data timeout
                            if last_data_time.elapsed() > Duration::from_secs(10) && packets_received == 0 {
                                let msg = "⚠️ No data for 10s. Check: 1) Device in CLIENT mode 2) Other nodes on 3) FW 2.2+";
                                eprintln!("{}", msg);
                                let _ = app_handle.emit("meshtastic-debug", msg);
                            }
                        }
                    }
                }
                
                eprintln!("Meshtastic disconnected - received {} packets", packets_received);
                let _ = app_handle.emit("meshtastic-status", "disconnected");
            }
            None => {
                eprintln!("Failed to open serial port at any baud rate");
                let error_msg = format!("Failed to open port {} at any baud rate (921600 or 115200)", port_clone);
                let _ = app_handle.emit("meshtastic-error", error_msg.clone());
                let _ = app_handle.emit("meshtastic-debug", error_msg);
            }
        }
        
        state_clone.is_connected.store(false, Ordering::SeqCst);
    });
    
    Ok(format!("Connecting to {}...", port))
}

fn send_protobuf<W: Write>(writer: &mut W, msg: ToRadio) -> Result<(), String> {
    let mut buf = Vec::new();
    msg.encode(&mut buf).map_err(|e| e.to_string())?;
    
    let len = buf.len();
    let mut packet = vec![START_BYTE, ((len >> 8) & 0xFF) as u8, (len & 0xFF) as u8, 0x00];
    packet.extend_from_slice(&buf);
    
    writer.write_all(&packet).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())?;
    
    Ok(())
}

fn handle_from_radio(app_handle: &tauri::AppHandle, state: &Arc<MeshtasticState>, from_radio: FromRadio) {
    use meshtastic_proto::from_radio::PayloadVariant;
    
    let _ = app_handle.emit("meshtastic-debug", "handle_from_radio called");
    
    match from_radio.payload_variant {
        Some(PayloadVariant::Packet(packet)) => {
            eprintln!("Received packet from node {}", packet.from);
            let _ = app_handle.emit("meshtastic-debug", format!("Packet from node {}", packet.from));
            handle_mesh_packet(app_handle, state, packet);
        }
        // Heartbeat removed in newer protobuf versions
        Some(PayloadVariant::ConfigCompleteId(_)) => {
            eprintln!("Config complete");
            let _ = app_handle.emit("meshtastic-status", "configured");
        }
        _ => {}
    }
}

fn handle_mesh_packet(app_handle: &tauri::AppHandle, state: &Arc<MeshtasticState>, packet: MeshPacket) {
    if packet.payload.is_empty() {
        return;
    }
    
    // Decode the data payload
    if let Ok(data) = Data::decode(&packet.payload[..]) {
        match data.portnum {
            // Text message
            1 => {
                if let Ok(text) = String::from_utf8(data.payload.clone()) {
                    let msg = MeshtasticMessage {
                        from: packet.from,
                        to: packet.to,
                        text,
                        timestamp: packet.rx_time as u64,
                        channel: packet.channel,
                    };
                    
                    {
                        let mut messages = state.messages.lock().unwrap();
                        messages.push(msg.clone());
                        if messages.len() > 100 {
                            messages.remove(0);
                        }
                    }
                    
                    let _ = app_handle.emit("meshtastic-message", msg);
                }
            }
            // Position
            3 => {
                if let Ok(pos) = Position::decode(&data.payload[..]) {
                    let position = MeshtasticPosition {
                        node_id: packet.from,
                        latitude: pos.latitude_i as f64 / 1e7,
                        longitude: pos.longitude_i as f64 / 1e7,
                        altitude: pos.altitude,
                        timestamp: pos.time,
                        precision: pos.precision_bits,
                    };
                    
                    {
                        let mut positions = state.positions.lock().unwrap();
                        positions.push(position.clone());
                        if positions.len() > 50 {
                            positions.remove(0);
                        }
                    }
                    
                    let _ = app_handle.emit("meshtastic-position", position);
                }
            }
            // Node info
            4 => {
                if let Ok(user) = User::decode(&data.payload[..]) {
                    let node = MeshtasticNode {
                        node_id: packet.from,
                        long_name: user.long_name,
                        short_name: user.short_name,
                        last_seen: Some(Instant::now()),
                        snr: 0.0,
                        hops_away: packet.hop_limit as u32,
                    };
                    
                    {
                        let mut nodes = state.nodes.lock().unwrap();
                        if let Some(existing) = nodes.iter_mut().find(|n| n.node_id == packet.from) {
                            existing.long_name = node.long_name.clone();
                            existing.short_name = node.short_name.clone();
                            existing.last_seen = node.last_seen;
                        } else {
                            nodes.push(node.clone());
                        }
                    }
                    
                    let _ = app_handle.emit("meshtastic-node", node);
                }
            }
            _ => {}
        }
    }
}

pub fn send_text_message(state: &Arc<MeshtasticState>, port: &str, text: String, channel: u32) -> Result<(), String> {
    use meshtastic_proto::to_radio::PayloadVariant;
    
    if !state.is_connected.load(Ordering::SeqCst) {
        return Err("Not connected to Meshtastic".to_string());
    }
    
    let my_node_id = state.my_node_id.lock().unwrap();
    let from = my_node_id.unwrap_or(0);
    drop(my_node_id);
    
    let data = Data {
        portnum: 1, // TEXT_MESSAGE_APP
        payload: text.into_bytes(),
        want_response: false,
        dest: 0,
        source: 0,
        request_id: vec![],
        reply_id: vec![],
    };
    
    let mut payload = Vec::new();
    data.encode(&mut payload).map_err(|e| e.to_string())?;
    
    let packet = MeshPacket {
        payload,
        from,
        to: 0xFFFFFFFF, // Broadcast
        channel,
        id: 0,
        rx_time: 0,
        rx_snr: 0,
        hop_limit: 3,
        want_ack: false,
        priority: 0, // UNSET
        rx_rssi: vec![],
        delayed: 0,
    };
    
    let to_radio = ToRadio {
        payload_variant: Some(PayloadVariant::Packet(packet)),
    };
    
    // Send via serial
    let mut serial_port = serialport::new(port, 921600)
        .timeout(Duration::from_millis(500))
        .open()
        .map_err(|e| e.to_string())?;
    
    send_protobuf(&mut serial_port, to_radio)
}

pub fn stop_meshtastic(state: &Arc<MeshtasticState>) {
    state.is_connected.store(false, Ordering::SeqCst);
}

pub fn get_available_ports() -> Vec<String> {
    let ports = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| {
            // Filter for common Meshtastic USB devices
            p.port_name.contains("usb") || 
            p.port_name.contains("USB") ||
            p.port_name.contains("ttyACM") ||
            p.port_name.contains("ttyUSB") ||
            p.port_name.contains("cu.usb")
        })
        .map(|p| p.port_name)
        .collect::<Vec<_>>();
    
    eprintln!("Found {} potential Meshtastic ports", ports.len());
    for port in &ports {
        eprintln!("  - {}", port);
    }
    
    ports
}