#[macro_use]
extern crate serde_derive;
extern crate serde;

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub struct ClientMessage {
    pub id: u32,
    pub name: String,
}

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub struct ServerMessage {
    pub id: u32,
    pub name: String,
}
