use serde::{Serialize, Deserialize};

// One snapshot of a patient's vitals sent by the wearable sensor.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SensorReading {
    // Heart rate in beats per minute
    pub hr: f32,

    // Skin temperature in degrees
    pub temp: f32,

    // Heart-rate variability — measures the spread between beat intervals
    pub hrv_sdnn: f32,

    // Heart-rate variability — measures how much consecutive beat gaps differ
    pub hrv_rmssd: f32,

    // When this reading was received by the server (filled in on arrival)
    #[serde(default)]
    pub timestamp: String,

    // Clock reading from the sensor at the moment it sent this data,
    // used to measure how long the message took to arrive
    #[serde(default)]
    pub esp_millis: u64,

    // How many heartbeats the sensor detected during this measurement window;
    // readings with fewer than 3 are too unreliable to use for predictions
    #[serde(default)]
    pub beat_count: u32,
}

// The package sent to the prediction model: a patient ID plus their last 12 readings.
#[derive(Debug, Serialize)]
pub struct ModelInput {
    pub patient_id: String,
    pub window: Vec<SensorReading>,
}
