use crate::{indexer::event::Event, Indexer};
use bus::BusReader;
use log::{error, trace};
use rusqlite::Connection;
use std::sync::{Arc, Mutex};
use std::thread;
use thiserror::Error;
use websocket::stream::sync::TcpStream;
use websocket::sync::Client;
use websocket::{sync::Server, Message};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Websocket error: {0}")]
    Websocket(#[from] std::io::Error),
}

/// Starts a background thread that implements websocket service for indexer
pub fn start_websocket_server(indexer: Arc<Indexer>, bind_addr: &str) -> Result<(), Error> {
    let server = Server::bind(bind_addr)?;
    // Listen new connections in new thread
    thread::spawn(move || {
        trace!("Spawn websocket server thread");
        for connection in server.filter_map(Result::ok) {
            let events_bus = match indexer.add_event_reader() {
                Err(e) => {
                    error!("Failed to get events bus for new connection: {e}");
                    continue;
                }
                Ok(v) => v,
            };
            let database = indexer.get_database().clone();

            // Spawn a new thread for each connection.
            trace!("New websocket connection");
            thread::spawn(move || {
                let mut client = match connection.accept() {
                    Err((stream, e)) => {
                        let addr = stream
                            .peer_addr()
                            .map_or("".to_owned(), |addr| addr.to_string());
                        error!("Failed to accept {addr} connection: {e}");
                        return;
                    }
                    Ok(client) => client,
                };
                let addr = client
                    .peer_addr()
                    .map_or("".to_owned(), |addr| addr.to_string());
                trace!("Handshaked with {addr}");
                match client_handler(&mut client, events_bus, database) {
                    Err(e) => {
                        error!("Connection with {addr} closed with error: {e}");
                    }
                    Ok(_) => {
                        trace!("Connection with {addr} closed normally");
                    }
                }
            });
        }
    });
    Ok(())
}

fn client_handler(
    client: &mut Client<TcpStream>,
    events_bus: BusReader<Event>,
    database: Arc<Mutex<Connection>>,
) -> Result<(), Error> {
    let message = Message::text("Hello, client!");
    let _ = client.send_message(&message);

    Ok(())
}
