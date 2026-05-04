use reqwest::Client;
use crate::sensor_readings::{SensorReading, ModelInput};

// Sends the last 12 sensor readings to the prediction model and prints the result.
// The secret key in the header proves this request is coming from our own service.
pub async fn send_batch_reading(buffer: &[SensorReading], host: &str, key: &str, patient_id: &str) {
    let client = Client::new();

    // Bundle the patient ID and the readings into a single package for the model
    let payload = ModelInput {
        patient_id: patient_id.to_string(),
        window: buffer.to_vec(),
    };

    // Send the package to the prediction model
    match client.post(host)
        .header("X-Internal-Key", key)  // proves the request is from this service
        .json(&payload)
        .send().await
    {
        Ok(response) => {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            println!("Response [{}]: {}", status, body);
        }
        Err(error) => {
            println!("Error sending request: {}", error);
        }
    }
}
