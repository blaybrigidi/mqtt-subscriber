# Vitals Subscriber

A background service that listens for real-time health readings from a wearable sensor and does two things with every reading: saves it to a cloud database, and — once enough readings have built up — forwards a batch to an AI prediction model.

This is part of the **Diallog** diabetes monitoring system. The sensor on the patient's body sends heart rate and temperature data continuously. This service is the always-on receiver that makes sure nothing gets lost and that the prediction model stays fed.

The service is deployed and hosted on **[Render](https://render.com)**, where it runs as a continual subscriber — meaning it stays connected and listening around the clock without needing to be manually started or restarted.

---

## What It Does

1. **Connects** to a secure cloud messaging server (HiveMQ) and listens on a channel dedicated to a specific patient.
2. **Receives** each incoming health reading and stamps it with the time it arrived.
3. **Saves** every reading to Firebase so there is always a persistent record.
4. **Filters** out low-quality readings — anything with fewer than 3 detected heartbeats in the window is discarded, since those readings produce unreliable values.
5. **Batches** the last 12 valid readings into a rolling window and sends them to the prediction model every time a new one arrives.

The saves and predictions happen in the background so that the service never falls behind on incoming data, even if the network is slow.

---

## System Requirements

- [Rust](https://www.rust-lang.org/tools/install) (stable, 1.70 or later)
- Internet access (to reach HiveMQ and Firebase)
- The AI prediction model running locally on port `8000`

To check if Rust is installed:

```bash
rustc --version
```

If it is not installed, run:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

---

## Getting Started

### 1. Clone the repository

```bash
git clone https://github.com/blaybrigidi/mqtt-subscriber.git
cd mqtt-subscriber
```

### 2. Create your environment file

The service reads all of its configuration from a `.env` file in the project root. A template is included in the repository:

```bash
cp .env.example .env
```

Then open `.env` and fill in the real values:

```env
RUST_TO_MODEL_KEY=your_secret_key_here

MQTT_HOST=your_hivemq_host_here
MQTT_PORT=8883
MQTT_CLIENT_ID=your_client_id_here
MQTT_USERNAME=your_mqtt_username_here
MQTT_PASSWORD=your_mqtt_password_here

USER_ID=your_patient_user_id_here

MODEL_URL=http://localhost:8000/predict
DATABASE_URL=https://your-project.firebasedatabase.app
```

> The `.env` file is listed in `.gitignore` and will never be committed to the repository. Keep it out of version control and never share it publicly.

A description of every variable is in the [Manual](MANUAL.md#environment-variables).

### 3. Build the project

```bash
cargo build --release
```

This downloads all dependencies and compiles the service. The first build takes a few minutes. Subsequent builds are much faster.

The compiled binary will be at:

```
target/release/mqtt-subscriber
```

### 4. Run the service

Make sure the prediction model is running on `http://localhost:8000/predict`, then start the subscriber:

```bash
cargo run --release
```

Or run the compiled binary directly:

```bash
./target/release/mqtt-subscriber
```

You should see output like:

```
Subscribed to topic 'vitals/GYj2b0AQmhZPECB7lAKLGNDYX2k2'
[RECV] hr=72.4 beat_count=8 esp_millis=1234567
Firebase write [200]: ...
Response [200]: {"prediction": ...}
```

---

## Project Structure

```
mqtt-subscriber/
├── src/
│   ├── main.rs              # Startup, connection setup, and main event loop
│   ├── sensor_readings.rs   # Data structures for a reading and a model batch
│   ├── firebase.rs          # Saves individual readings to the cloud database
│   └── model_connector.rs   # Sends batches of readings to the prediction model
├── Cargo.toml               # Project dependencies
├── .env                     # Secret credentials (not committed to git)
├── README.md                # This file
└── MANUAL.md                # Full operational manual
```

---

## External Services

| Service | What it does |
|---|---|
| Render | Hosts and runs this service continuously in the cloud |
| HiveMQ Cloud | The messaging server the sensor publishes to — this service listens here |
| Firebase Realtime Database | Cloud storage where every reading is permanently saved |
| Local ML model (`localhost:8000`) | The prediction model that analyses batches of readings |

---

## Stopping the Service

Press `Ctrl + C` in the terminal where it is running. The service will stop immediately. Any readings that were in-flight at that moment (being written to Firebase or sent to the model) may not complete.

---

## Further Reading

For a deeper explanation of how the service works, how to troubleshoot it, and what each log message means, see the **[Manual](MANUAL.md)**.
