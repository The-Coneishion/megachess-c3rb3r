use std::{collections::HashMap, io};
use serde_json;
use tokio::sync::mpsc;
use serde::{Deserialize, Serialize};
use futures::{SinkExt, StreamExt, TryStreamExt, lock::Mutex, stream};
use tokio::{net::{TcpListener, TcpStream}};

use tokio_tungstenite::{WebSocketStream, accept_hdr_async, tungstenite::{Message, http::{Request, Response}}};

pub struct ConnectionInfo {
    addr: String,
}

impl ConnectionInfo {
    pub fn new(addr: &str, port: u16) -> ConnectionInfo {
        ConnectionInfo {
            addr: format!("{}:{}", addr, port),
        }
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }
}

pub struct Server {
    listener: TcpListener,
}

impl Server {
    pub async fn new(conn_info: ConnectionInfo) -> Result<Server, io::Error> {
        let listener = TcpListener::bind(conn_info.addr()).await?;
        println!("Server started and listening in: {}", conn_info.addr());
        Ok(Server {
            listener,
        })
    }

    async fn auth_token(&self, token: &str) -> bool {
        !token.is_empty()
    }

    pub async fn run(&self, game_manager: &GameManager) -> Result<(), io::Error> {
        while let Ok((stream, addr)) = self.listener.accept().await {
            println!("Received connection from {}", addr);
            let mut query_split: HashMap<String, String> = HashMap::new();
            let mut path = String::new();
            let ws_stream = accept_hdr_async(stream, |req: &Request<()>, res: Response<()>| {
                path = req.uri().path().to_string();
                let query_params: Vec<&str> = req.uri().query().unwrap_or_default().split("&").collect();
                for qp in query_params {
                    let qs: Vec<&str> = qp.split("=").collect();
                    query_split.insert(String::from(qs[0]), String::from(qs[1]));
                }
                Ok(res)
            }).await.unwrap();

            if let Some(token) = query_split.get("authtoken") {
                if path != "/service" || !self.auth_token(&token).await {
                    continue;
                }
                println!("Opened websocket for {} with token: {}", addr, &token);
                let (out, mut inc) = ws_stream.split();
                
                let user = String::from("token.user");
                game_manager.add_client(user.clone(), out).await;
                let client_ch = game_manager.new_client_channel();
    
                tokio::spawn(async move {
                    while let Some(msg) = inc.try_next().await.unwrap() {
                        if let Ok(client_action) = MessageHandler::parse_message(msg.to_string()) {
                            let client_msg = IDClientMessage{user: user.clone(), client_action};
                            client_ch.send(client_msg).await.unwrap();
                        }
                    }
                    println!("Closed connection");
                });
            }
        }
        Ok(())
    }
}

struct MessageHandler;

impl MessageHandler {
    pub fn parse_message(msg: String) -> Result<ClientMessage, serde_json::Error> {
        serde_json::from_str::<ClientMessage>(&msg)
    }
}

#[derive(Debug)]
pub struct IDClientMessage {
    user: String,
    client_action: ClientMessage,
}

#[derive(Debug, Deserialize)]
pub struct ClientMessage {
    action: String,
    data: String,
}

#[derive(Debug, Serialize)]
pub struct ServerMessage {
    event: String,
    data: String,
}

pub struct GameManager {
    server_ch: Mutex<mpsc::Receiver<IDClientMessage>>,
    client_ch: mpsc::Sender<IDClientMessage>,
    client_list: Mutex<HashMap<
        String,
        stream::SplitSink<WebSocketStream<TcpStream>, Message>
    >>,
}

impl GameManager {
    pub fn new() -> GameManager {
        let (client_ch, server_ch) = mpsc::channel::<IDClientMessage>(20);
        GameManager {
            server_ch: Mutex::new(server_ch),
            client_ch,
            client_list: Mutex::new(HashMap::new()),
        }
    }

    pub async fn add_client(&self, user: String, ws: stream::SplitSink<WebSocketStream<TcpStream>, Message>) {
        self.client_list.lock().await.insert(user, ws);
    }

    pub fn new_client_channel(&self) -> mpsc::Sender<IDClientMessage> {
        self.client_ch.clone()
    }

    pub async fn run(&self) {
        while let Some(msg) = self.server_ch.lock().await.recv().await {
            // Prepare server response
            let server_msg = ServerMessage{event: msg.client_action.action, data: msg.client_action.data};
            let server_msg_ser = serde_json::to_string(&server_msg).unwrap();
            // Send response to client
            let mut client_ws_lock = self.client_list.lock().await;
            let client_ws = client_ws_lock.get_mut(&msg.user).unwrap();
            client_ws.send(Message::from(server_msg_ser)).await.unwrap();
        }
    }
}
