/*
op codes cheat sheet:
0: gateway event
1: heartbeat sent
2: ready event (A load of info like guilds, user, settings, etc)
10: discord sent you heartbeat interval, hello
11: discord received your heartbeat

The gateway events are identified by string names
VC has its own op codes

btw people's email address is public through the api I think, weird
*/

use url::Url;
use tokio_tungstenite::tungstenite::{connect, Message, WebSocket};
use tokio_tungstenite::tungstenite::stream::MaybeTlsStream;
use std::net::TcpStream;
use std::time::Instant;
use serde_json::{self, Value};
use std::thread;
use std::sync::mpsc;

use crate::api::data::*;

pub fn start_thread(token: &String) -> mpsc::Receiver<GatewayResponse> {
    let (tx, rx) = mpsc::channel();
    let token_clone = token.clone(); // Clone the token here

    thread::spawn(move || {
        loop {
            let gateway_url = "wss://gateway.discord.gg/?v=9&encoding=json";
            let (mut socket, _response) = connect(
                Url::parse(gateway_url).unwrap()
            ).expect("Can't connect");

            let handshake = read_json_event(&mut socket).unwrap();
            let hb_interval = handshake["d"]["heartbeat_interval"].as_u64().unwrap();
            //println!("Received Hbeat: {}", hb_interval);

            identify(&mut socket, &token_clone);

            let ready = read_json_event(&mut socket).expect("Couldn't get ready event");
            ready_event(&tx, ready);

            // Notify the main thread that a reconnection occurred
            tx.send(GatewayResponse::reconnected()).expect("Failed to send reconnected message");

            let mut last_heartbeat = Instant::now();

            loop {
                if last_heartbeat.elapsed().as_millis() as u64 >= hb_interval {
                    heartbeat(&mut socket);
                    last_heartbeat = Instant::now();
                }

                match read_json_event(&mut socket) {
                    Ok(event) => {
                        let op_code = event["op"].as_i64().unwrap();
                        if op_code == 1 {
                            heartbeat(&mut socket);
                        } else if op_code == 0 {
                            let event_name = event["t"].as_str().unwrap();
                            match event_name {
                                "MESSAGE_CREATE" => { message_created(&tx, &event); },
                                "MESSAGE_REACTION_ADD" => (),
                                "MESSAGE_REACTION_REMOVE" => (),
                                "TYPING_START" => (),
                                "CHANNEL_CREATE" => (),
                                "GUILD_CREATE" => (),
                                "GUILD_DELETE" => (),
                                _ => ()
                            }
                        }
                    },
                    Err(ReadJsonEventError::WebSocketError(tungstenite::error::Error::ConnectionClosed)) => {
                        //println!("Connection closed by the server. Reconnecting...");
                        break;
                    },
                    Err(ReadJsonEventError::WebSocketError(tungstenite::error::Error::AlreadyClosed)) => {
                        //println!("Connection already closed.");
                        break;
                    },
                    Err(e) => {
                        //println!("Error: {:?}", e);
                        break;
                    }
                }
            }

            //println!("Reconnecting...");
        }
    });

    return rx;
}

fn heartbeat(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>) {
    let reply = Message::Text(r#"{
        "op": 1,
        "d": null
    }"#.into());

    socket.write_message(reply).expect("Hbeat failed");
}

fn identify(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>, token: &str) {
    let reply = format!("{{
        \"op\": 2,
        \"d\": {{
            \"token\": \"{}\",
            \"properties\": {{
                \"$os\": \"linux\",
                \"$browser\": \"chrome\",
                \"$device\": \"pc\"
            }}
        }}
    }}", token);

    let reply = Message::Text(reply.into());

    socket.write_message(reply).expect("Identification failed");
}

// Assume message_created and ready_event are unchanged


//Makes a Msg object and sends it back to ui thread
fn message_created(tx: &mpsc::Sender<GatewayResponse>, event: &Value) {
    let msg = Msg::from(&event["d"]);
    let gate_response = GatewayResponse::msg_create(msg);
    tx.send(gate_response).unwrap();
}

fn ready_event(tx: &mpsc::Sender<GatewayResponse>, event: Value) {
    let guilds = Guild::from_list(&event["d"]);
    let gate_response = GatewayResponse::ready(guilds);
    tx.send(gate_response).unwrap();
}

fn read_json_event(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>) -> Result<Value, ReadJsonEventError> {
    let msg = socket.read_message()?;
    let text_msg = msg.to_text().map_err(|_| ReadJsonEventError::ProtocolError("Non-text message".into()))?;
    let json_msg = serde_json::from_str(text_msg)?;
    Ok(json_msg)
}

#[derive(Debug)]
enum ReadJsonEventError {
    WebSocketError(tungstenite::error::Error),
    JsonError(serde_json::Error),
    ProtocolError(String),
}

impl From<tungstenite::error::Error> for ReadJsonEventError {
    fn from(err: tungstenite::error::Error) -> Self {
        ReadJsonEventError::WebSocketError(err)
    }
}

impl From<serde_json::Error> for ReadJsonEventError {
    fn from(err: serde_json::Error) -> Self {
        ReadJsonEventError::JsonError(err)
    }
}