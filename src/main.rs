use rumqttc::{MqttOptions, AsyncClient, Event, Packet, QoS, Transport};
use chrono::Local;
use reqwest::Client;
use std::time::Duration;

mod sensor_readings;
use sensor_readings::SensorReading;

mod model_connector;
use model_connector::send_batch_reading;

mod firebase;
use firebase::send_to_firebase;

#[tokio::main]
async fn main() {
    // Load all configuration values from the .env file
    dotenvy::dotenv().ok();

    // The address of the AI prediction service running locally
    let model_url = std::env::var("MODEL_URL").expect("MODEL_URL must be set");

    // The address of the cloud database where readings are stored
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    // The patient whose sensor data we are listening for
    let user_id = std::env::var("USER_ID").expect("USER_ID must be set");

    // A secret key that proves this service is allowed to talk to the prediction model
    let internal_key = std::env::var("RUST_TO_MODEL_KEY").expect("RUST_TO_MODEL_KEY must be set");

    // Credentials to log in to the messaging server
    let mqtt_username = std::env::var("MQTT_USERNAME").expect("MQTT_USERNAME must be set");
    let mqtt_password = std::env::var("MQTT_PASSWORD").expect("MQTT_PASSWORD must be set");
    let mqtt_host = std::env::var("MQTT_HOST").expect("MQTT_HOST must be set");
    let mqtt_client_id = std::env::var("MQTT_CLIENT_ID").expect("MQTT_CLIENT_ID must be set");

    // A rolling list of the most recent sensor readings, used to feed the prediction model
    let mut buffer: Vec<SensorReading> = Vec::new();

    // One shared HTTP client reused for every Firebase write and model call
    let http_client = Client::new();

    // Set up the connection details for the messaging server (HiveMQ cloud)
    let mut mqttoptions = MqttOptions::new(mqtt_client_id, mqtt_host, 8883);

    // Send a heartbeat every 30 seconds so the server knows we are still connected
    mqttoptions.set_keep_alive(Duration::from_secs(30));

    // Resume any missed messages when we reconnect after a drop
    mqttoptions.set_clean_session(false);

    mqttoptions.set_credentials(mqtt_username, mqtt_password);

    // Use an encrypted connection
    mqttoptions.set_transport(Transport::Tls(Default::default()));

    // Open the connection and get a handle to send/receive messages
    let (client, mut eventloop) = AsyncClient::new(mqttoptions, 10);

    // The channel we listen on — scoped to this specific patient
    let topic = format!("vitals/{}", user_id);

    // Start listening on the patient's channel
    if let Err(e) = client.subscribe(&topic, QoS::AtLeastOnce).await {
        eprintln!("Failed to subscribe: {:?}", e);
        return;
    }

    println!("Subscribed to topic '{}'", topic);

    // Keep running forever, processing one incoming event at a time
    loop {
        match eventloop.poll().await {
            Ok(event) => {
                // Log when we (re)connect to the messaging server
                if let Event::Incoming(Packet::ConnAck(_)) = &event {
                    println!("[MQTT] Reconnected to broker");
                }

                // A new sensor reading has arrived
                if let Event::Incoming(Packet::Publish(p)) = event {
                    // Try to parse the raw bytes into a structured reading
                    let reading: SensorReading = match serde_json::from_slice(&p.payload) {
                        Ok(r) => r,
                        Err(e) => {
                            eprintln!("Bad payload, skipping: {:?}", e);
                            continue;
                        }
                    };

                    // Stamp the reading with the current server time
                    let mut reading = reading;
                    reading.timestamp = Local::now().to_rfc3339();
                    println!("[RECV] hr={:.1} beat_count={} esp_millis={}", reading.hr, reading.beat_count, reading.esp_millis);

                    // Save the reading to the cloud database in the background
                    // (runs separately so it doesn't slow down receiving the next reading)
                    let reading_clone = reading.clone();
                    let db_url_clone = database_url.clone();
                    let user_id_clone = user_id.clone();
                    let http_client_clone = http_client.clone();
                    tokio::spawn(async move {
                        send_to_firebase(&http_client_clone, &reading_clone, &db_url_clone, &user_id_clone).await;
                    });

                    // Only keep readings where the sensor detected at least 3 heartbeats.
                    // Fewer than 3 beats means the heart-rate variability numbers are stale
                    // and would confuse the prediction model.
                    if reading.beat_count < 3 {
                        println!("[FILTER] Dropped low-quality reading (beat_count={})", reading.beat_count);
                        continue;
                    }

                    // Add the reading to our rolling window
                    buffer.push(reading);

                    // Keep only the most recent 12 readings
                    if buffer.len() > 12 {
                        buffer.remove(0);
                    }

                    // Once we have exactly 12 readings, send them to the prediction model
                    if buffer.len() == 12 {
                        let batch = buffer.clone();
                        let model_url_clone = model_url.clone();
                        let key_clone = internal_key.clone();
                        let patient_id_clone = user_id.clone();
                        let http_client_clone = http_client.clone();

                        // Run the prediction in the background so we can keep receiving data
                        tokio::spawn(async move {
                            send_batch_reading(&http_client_clone, &batch, &model_url_clone, &key_clone, &patient_id_clone).await;
                        });
                    }
                }
            }
            Err(e) => {
                eprintln!("MQTT error: {:?}", e);
            }
        }
    }
}
