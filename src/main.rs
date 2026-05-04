use rumqttc::{MqttOptions, AsyncClient, Event, Packet, QoS, Transport};
use chrono::Local;
use std::time::Duration;

mod sensor_readings;
use sensor_readings::SensorReading;

mod model_connector;
use model_connector::send_batch_reading;

mod firebase;
use firebase::send_to_firebase;

#[tokio::main]
async fn main() {
    // Load secret values (like the internal key) from the .env file
    dotenvy::dotenv().ok();

    // The address of the AI prediction service running locally
    let model_url = "http://localhost:8000/predict";

    // The address of the cloud database where readings are stored
    let database_url = "https://diallog-78c08-default-rtdb.europe-west1.firebasedatabase.app";

    // A rolling list of the most recent sensor readings, used to feed the prediction model
    let mut buffer: Vec<SensorReading> = Vec::new();

    // The patient whose sensor data we are listening for
    let user_id = "GYj2b0AQmhZPECB7lAKLGNDYX2k2";

    // A secret key that proves this service is allowed to talk to the prediction model
    let internal_key = std::env::var("RUST_TO_MODEL_KEY")
        .expect("RUST_TO_MODEL_KEY must be set");

    // Set up the connection details for the messaging server (HiveMQ cloud)
    let mut mqttoptions = MqttOptions::new(
        "rust-vitals-subscriber-02",
        "35048523647747189040301dcfbe034d.s1.eu.hivemq.cloud",
        8883,
    );

    // Send a heartbeat every 30 seconds so the server knows we are still connected
    mqttoptions.set_keep_alive(Duration::from_secs(30));

    // Resume any missed messages when we reconnect after a drop
    mqttoptions.set_clean_session(false);

    // Credentials to log in to the messaging server
    mqttoptions.set_credentials("ama_annor", "Amaannorrocks12");

    // Use an encrypted connection
    mqttoptions.set_transport(Transport::Tls(Default::default()));

    // Open the connection and get a handle to send/receive messages
    let (client, mut eventloop) = AsyncClient::new(mqttoptions, 10);

    // The channel we listen on — scoped to this specific patient
    let topic = "vitals/GYj2b0AQmhZPECB7lAKLGNDYX2k2";

    // Start listening on the patient's channel
    if let Err(e) = client.subscribe(topic, QoS::AtLeastOnce).await {
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
                    let db_url_clone = database_url.to_string();
                    let user_id_clone = user_id.to_string();
                    tokio::spawn(async move {
                        send_to_firebase(&reading_clone, &db_url_clone, &user_id_clone).await;
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
                        let model_url_clone = model_url.to_string();
                        let key_clone = internal_key.clone();
                        let patient_id_clone = user_id.to_string();

                        // Run the prediction in the background so we can keep receiving data
                        tokio::spawn(async move {
                            send_batch_reading(&batch, &model_url_clone, &key_clone, &patient_id_clone).await;
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
