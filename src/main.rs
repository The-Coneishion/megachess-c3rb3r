use std::process::exit;

use futures::{future::select, pin_mut};

mod server;

#[tokio::main]
pub async fn main() {
    let conn_info = server::ConnectionInfo::new("127.0.0.1", 8080);
    let game_manager = server::GameManager::new();

    let conn_server = server::Server::new(conn_info).await.unwrap_or_else(|e| {
        println!("Error while creating server: {}",e);
        exit(1);
    });


    // conn_server.run(&game_manager);
    // game_manager.run();
    let cs_fut = conn_server.run(&game_manager);
    let gm_fut = game_manager.run();

    pin_mut!(cs_fut);
    pin_mut!(gm_fut);

    select(cs_fut, gm_fut).await;
}
