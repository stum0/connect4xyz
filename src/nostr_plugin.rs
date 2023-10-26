use std::sync::{Arc, Mutex};

use nanoid::nanoid;

use wasm_bindgen::JsValue;
use web_sys::{window, History};

use bevy::prelude::*;
use futures::StreamExt;
use nostr_sdk::{
    serde_json, Client, ClientMessage, EventBuilder, Filter, RelayPoolNotification, Tag, Timestamp,
};
use wasm_bindgen_futures::spawn_local;

use crate::{
    components::{CoinMove, ReplayButton},
    gui_plugin::check_player_connection_and_hide_button,
    messages::{NetworkMessage, Players},
    resources::{Board, NetworkStuff, PlayerMove, SendNetMsg},
    AppState,
};

const COIN_SIZE: Vec2 = Vec2::new(40.0, 40.0);
const COLUMNS: usize = 7;
const ROWS: usize = 7;
const SPACING: f32 = 5.0;

pub struct NostrPlugin;

impl Plugin for NostrPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(NetworkStuff::new())
            .insert_resource(SendNetMsg::new())
            .add_systems(OnEnter(AppState::InGame), setup)
            .add_systems(Update, handle_net_msg.run_if(in_state(AppState::InGame)));
    }
}

fn setup(mut network_stuff: ResMut<NetworkStuff>, mut send_net_msg: ResMut<SendNetMsg>) {
    let (send_tx, send_rx) = futures::channel::mpsc::channel::<String>(1000);
    let (nostr_msg_tx, mut nostr_msg_rx) = futures::channel::mpsc::channel::<ClientMessage>(1000);

    let location = web_sys::window().unwrap().location();
    let tag = location.pathname().unwrap().to_string();
    send_net_msg.game_tag = Tag::Hashtag(tag.clone());

    let send_net_msg_clone = send_net_msg.clone();

    network_stuff.read = Some(send_rx);
    send_net_msg.send = Some(nostr_msg_tx);

    spawn_local(async move {
        let client = Client::new(&send_net_msg_clone.nostr_keys);

        #[cfg(target_arch = "wasm32")]
        client.add_relay("wss://relay.damus.io").await.unwrap();

        // #[cfg(target_arch = "wasm32")]
        // client.add_relay("wss://relay.snort.social").await.unwrap();

        //doesnt support something
        // #[cfg(target_arch = "wasm32")]
        // client.add_relay("wss://nostr.lu.ke").await.unwrap();
        client.connect().await;

        let client_clone = client.clone();

        spawn_local(async move {
            while let Some(msg) = nostr_msg_rx.next().await {
                client_clone.clone().send_msg(msg).await.unwrap();
            }
        });

        let subscription = Filter::new().hashtag(tag.clone());

        client.subscribe(vec![subscription]).await;

        client
            .handle_notifications(|notification| async {
                if let RelayPoolNotification::Event(_url, event) = notification {
                    match send_tx.clone().try_send(event.content.clone()) {
                        Ok(()) => {}
                        Err(e) => error!("Error sending message: {} CHANNEL FULL???", e),
                    };
                }
                Ok(false)
            })
            .await
            .unwrap();
    });
}

fn handle_net_msg(
    mut network_stuff: ResMut<NetworkStuff>,
    mut send_net_msg: ResMut<SendNetMsg>,
    mut board: ResMut<Board>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    coin_query: Query<Entity, With<CoinMove>>,
    mut replay_button: Query<(&mut ReplayButton, &mut Visibility)>,
) {
    if send_net_msg.created_game {
        send_net_msg.clone().new_game();
        send_net_msg.created_game = false;
    }

    if let Some(ref mut receive_rx) = network_stuff.read {
        while let Ok(Some(message)) = receive_rx.try_next() {
            match serde_json::from_str::<NetworkMessage>(&message) {
                Ok(network_message) => match network_message {
                    NetworkMessage::NewGame => {
                        if send_net_msg.start {
                            return;
                        }
                        info!("received new game msg ");
                        send_net_msg.clone().join_game();
                    }
                    NetworkMessage::JoinGame(other_player) => {
                        if send_net_msg.start {
                            return;
                        }
                        let players = Players::new(send_net_msg.local_player, other_player);
                        send_net_msg.clone().start_game(players);
                        send_net_msg.start = true;
                        send_net_msg.player_type = 1;
                        info!("received join game msg {:?}", send_net_msg.player_type);
                    }
                    NetworkMessage::StartGame(players) => {
                        if send_net_msg.start {
                            return;
                        }

                        if send_net_msg.local_player != players.player1
                            && send_net_msg.local_player != players.player2
                        {
                            send_net_msg.player_type = 3;
                            info!(
                                "received start game msg {:?}, {:?}",
                                players, send_net_msg.player_type
                            );
                            send_net_msg.start = true;
                            return;
                        }

                        send_net_msg.start = true;
                        send_net_msg.player_type = 2;
                        info!(
                            "received start game msg {:?}, {:?}",
                            players, send_net_msg.player_type
                        );
                    }

                    NetworkMessage::Input(new_input) => {
                        info!("received input {:?}", new_input);

                        let row_pos = board.moves.iter().filter(|m| m.column == new_input).count();
                        if row_pos <= 5 {
                            let player_move =
                                PlayerMove::new(board.player_turn, new_input, row_pos);

                            info!("receiv player move {:?}", player_move);

                            board.moves.push(player_move);

                            let offset_x = -COIN_SIZE.x * (COLUMNS as f32) / 2.0;
                            let offset_y = -COIN_SIZE.y * (ROWS as f32) / 2.0;

                            if board.player_turn == 1 {
                                commands
                                    .spawn(SpriteBundle {
                                        sprite: Sprite {
                                            custom_size: Some(COIN_SIZE),
                                            ..Default::default()
                                        },
                                        texture: asset_server.load("red_circle.png"),
                                        transform: Transform::from_xyz(
                                            offset_x + new_input as f32 * (COIN_SIZE.x + SPACING),
                                            offset_y + 6_f32 * (COIN_SIZE.y + SPACING),
                                            1.0,
                                        ),
                                        ..Default::default()
                                    })
                                    .insert(CoinMove::new(player_move));
                            } else {
                                commands
                                    .spawn(SpriteBundle {
                                        sprite: Sprite {
                                            custom_size: Some(COIN_SIZE),
                                            ..Default::default()
                                        },
                                        texture: asset_server.load("yellow_circle.png"),
                                        transform: Transform::from_xyz(
                                            offset_x + new_input as f32 * (COIN_SIZE.x + SPACING),
                                            offset_y + 6_f32 * (COIN_SIZE.y + SPACING),
                                            1.0,
                                        ),
                                        ..Default::default()
                                    })
                                    .insert(CoinMove::new(player_move));
                            }

                            break;
                        }
                    }
                    NetworkMessage::Replay => {
                        *board = Board::new();
                        for entity in coin_query.iter() {
                            commands.entity(entity).despawn();
                        }
                        for (_, mut visibility) in replay_button.iter_mut() {
                            *visibility = Visibility::Hidden;
                        }
                    }
                },

                Err(e) => {
                    info!("Failed to deserialize message: {:?}", e);
                }
            }
        }
    }
}
