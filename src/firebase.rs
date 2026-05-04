use reqwest::Client;
use uuid::Uuid;
use crate::sensor_readings::SensorReading;

// Saves a single sensor reading to the cloud database under the patient's folder.
// Each reading gets its own unique ID so entries never overwrite each other.
pub async fn send_to_firebase(client: &Client, reading: &SensorReading, host_url: &str, user_id: &str) {
    // Generate a unique ID for this reading so it gets its own slot in the database
    let reading_id = Uuid::new_v4().to_string();

    // Build the full database path: /patient_data/<user>/<unique-id>
    let url = format!("{}/patient_data/{}/{}.json", host_url, user_id, reading_id);

    // Send the reading and report whether it was saved successfully
    match client.put(&url).json(reading).send().await {
        Ok(res) => {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            println!("Firebase write [{}]: {}", status, body);
        }
        Err(e) => {
            eprintln!("Firebase write error: {:?}", e);
        }
    }
}
