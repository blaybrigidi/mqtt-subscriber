# Vitals Subscriber — Operational Manual

This manual covers everything you need to run, understand, and maintain the Vitals Subscriber service. If you are setting up for the first time, start with the [README](README.md) and come back here once the service is running.

---

## Table of Contents

1. [How the System Works](#how-the-system-works)
2. [Environment Variables](#environment-variables)
3. [Understanding the Logs](#understanding-the-logs)
4. [Data Flow Explained](#data-flow-explained)
5. [The 12-Reading Window](#the-12-reading-window)
6. [The Quality Filter](#the-quality-filter)
7. [Deploying to a Server](#deploying-to-a-server)
8. [Running as a Background Service](#running-as-a-background-service)
9. [Troubleshooting](#troubleshooting)

---

## How the System Works

The sensor on the patient's wrist (or elsewhere on the body) continuously measures heart rate, skin temperature, and heart-rate variability. It publishes these readings over the internet to a cloud messaging server (HiveMQ). This service sits in the middle — it subscribes to the patient's dedicated channel on that server and reacts to every reading that comes in.

Here is the full journey of a single reading:

```
Wearable sensor
      │
      │  (publishes to HiveMQ over the internet)
      ▼
HiveMQ Cloud Broker
      │
      │  (this service receives it)
      ▼
Vitals Subscriber
      ├──► Firebase (saved immediately, every reading)
      └──► Prediction Model (sent in batches of 12)
```

The service does not make predictions itself. It is purely a data pipeline — its job is to receive, store, and forward.

---

## Environment Variables

All configuration is read from a `.env` file in the project root. Copy `.env.example` to `.env` and fill in the values. The file is never committed to the repository.

| Variable | Description |
|---|---|
| `RUST_TO_MODEL_KEY` | A secret key sent with every request to the prediction model to prove the request is coming from this service and not an outside caller. |
| `MQTT_HOST` | The address of the HiveMQ cloud messaging server. |
| `MQTT_PORT` | The port to connect on — `8883` is standard for encrypted MQTT. |
| `MQTT_CLIENT_ID` | A unique name for this connection on the messaging server. |
| `MQTT_USERNAME` | Username to log in to the messaging server. |
| `MQTT_PASSWORD` | Password to log in to the messaging server. |
| `USER_ID` | The patient's unique ID — used to subscribe to their channel and store readings under their folder in the database. |
| `MODEL_URL` | The full address of the prediction model endpoint (e.g. `http://localhost:8000/predict`). |
| `DATABASE_URL` | The base URL of the Firebase Realtime Database where readings are stored. |

---

## Understanding the Logs

The service prints a line to the terminal for every significant event. Here is what each one means:

---

**`Subscribed to topic 'vitals/...'`**

The service has connected to HiveMQ and is now listening. This appears once at startup (and again after any reconnection).

---

**`[MQTT] Reconnected to broker`**

The connection to HiveMQ dropped and was automatically restored. This is normal — network hiccups happen. The service handles reconnections on its own.

---

**`[RECV] hr=72.4 beat_count=8 esp_millis=1234567`**

A reading arrived. The values shown are:
- `hr` — heart rate in beats per minute
- `beat_count` — how many heartbeats the sensor detected in its measurement window (used by the quality filter — see below)
- `esp_millis` — a timestamp from the sensor itself, useful for measuring how long the message took to travel from the device to here

---

**`[FILTER] Dropped low-quality reading (beat_count=3)`**

The reading had too few detected heartbeats to produce reliable values. It was saved to Firebase but not added to the prediction window. See [The Quality Filter](#the-quality-filter) for more detail.

---

**`Firebase write [200]: ...`**

The reading was successfully saved to the cloud database. The number in brackets is the HTTP response code — `200` means success.

If you see `Firebase write error: ...` instead, the database was unreachable. The reading is not retried; it will be missing from the database.

---

**`Response [200]: {"prediction": ...}`**

The prediction model received the 12-reading batch and responded. The number in brackets is the HTTP response code and the JSON shows the model's output.

If you see `Error sending request: ...`, the prediction model was not reachable. The batch is not retried.

---

**`MQTT error: ...`**

Something went wrong with the connection to HiveMQ. The service will keep running and attempt to reconnect automatically.

---

## Data Flow Explained

### On arrival

Every reading that comes in goes through these steps in order:

1. The raw bytes are decoded into a structured reading (heart rate, temperature, HRV values, beat count).
2. The current server time is attached to the reading as a timestamp.
3. The reading is sent to Firebase. This happens immediately and the service waits for it to complete before moving on.
4. If the reading has fewer than 3 detected heartbeats, it is discarded from the prediction pipeline (but it was already saved to Firebase in step 3).
5. The reading is added to the rolling 12-reading window.
6. If the window is full (12 readings), a copy of it is sent to the prediction model in the background.

### Why saves are immediate but predictions are not

Saving to Firebase happens before anything else so that no readings are ever lost. Even if the prediction model is down, the database record is created.

The prediction model call, on the other hand, runs in the background. This means the service can receive the next reading while the previous prediction is still in progress. If predictions were blocking, a slow model response could cause the service to fall behind on incoming sensor data.

---

## The 12-Reading Window

The prediction model does not analyse individual readings in isolation — it needs context. It looks at a sequence of 12 readings to detect trends (is the heart rate rising? is HRV declining over the last few minutes?).

The service keeps a rolling list of the last 12 valid readings in memory. Every time a new reading arrives and passes the quality filter, it is added to the end of the list and the oldest one falls off. Once the list has exactly 12 readings, a copy is sent to the model.

This means the model is called on every single incoming reading (not every 12th reading). The model always gets the most up-to-date window available.

```
Window after reading #12:  [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]  → model called
Window after reading #13:  [2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13] → model called
Window after reading #14:  [3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14] → model called
```

The window resets if the service restarts. It takes the first 12 valid readings to arrive before predictions resume.

---

## The Quality Filter

Each reading includes a `beat_count` field — how many heartbeats the sensor physically detected during its measurement window.

Heart-rate variability (HRV) is calculated from the gaps between consecutive heartbeats. If the sensor only detected 1 or 2 heartbeats, it cannot calculate a meaningful HRV and will report stale or recycled values instead. Sending those values to the prediction model would corrupt the rolling window with bad data.

The filter drops any reading with fewer than 3 detected beats. These readings are still saved to Firebase so the raw data is preserved, but they do not enter the prediction pipeline.

---

## Deploying to a Server

If you want this service to run on a remote machine (for example a cloud VM or a Raspberry Pi), follow these steps.

### Build for the target machine

If you are building on the same machine you will deploy to:

```bash
cargo build --release
```

If you are cross-compiling (building on a Mac to run on a Linux server), you will need to install a cross-compilation target. This is an advanced topic — the simplest approach is to build directly on the target machine.

### Copy the binary

```bash
scp target/release/mqtt-subscriber user@your-server:/home/user/mqtt-subscriber
```

### Copy the environment file

```bash
scp .env user@your-server:/home/user/.env
```

Make sure the `.env` file is in the same directory you will run the binary from.

### Make sure the prediction model is running

The prediction model must be running on the same machine, listening on port `8000`, before you start this service.

---

## Running as a Background Service

On Linux systems, you can use `systemd` to run the service automatically in the background and restart it if it crashes.

Create a service file at `/etc/systemd/system/vitals-subscriber.service`:

```ini
[Unit]
Description=Vitals Subscriber — MQTT to Firebase and prediction model
After=network.target

[Service]
Type=simple
User=your-username
WorkingDirectory=/home/your-username
ExecStart=/home/your-username/mqtt-subscriber
Restart=on-failure
RestartSec=5
EnvironmentFile=/home/your-username/.env

[Install]
WantedBy=multi-user.target
```

Then enable and start it:

```bash
sudo systemctl daemon-reload
sudo systemctl enable vitals-subscriber
sudo systemctl start vitals-subscriber
```

Check that it is running:

```bash
sudo systemctl status vitals-subscriber
```

View the live logs:

```bash
journalctl -u vitals-subscriber -f
```

---

## Troubleshooting

**The service starts but I see no `[RECV]` lines**

The service is connected but no readings are arriving. Check that:
- The sensor/publishing device is powered on and connected to the internet
- The sensor is publishing to the correct topic (`vitals/<patient-id>`)
- The `USER_ID` in your `.env` matches the patient ID the sensor is publishing under

---

**`RUST_TO_MODEL_KEY must be set` error on startup**

The `.env` file is missing or does not contain the `RUST_TO_MODEL_KEY` variable. Make sure the file exists in the directory where you are running the binary and that it contains:

```env
RUST_TO_MODEL_KEY=your_key_here
```

---

**`Failed to subscribe` on startup**

The service could not connect to HiveMQ. Check:
- Your internet connection
- That `MQTT_HOST`, `MQTT_USERNAME`, and `MQTT_PASSWORD` in your `.env` are correct
- That port `8883` is not blocked by a firewall

---

**Firebase writes are failing (`Firebase write error: ...`)**

- Check your internet connection
- Verify the `DATABASE_URL` in your `.env` is correct
- Confirm the Firebase Realtime Database security rules allow writes to `/patient_data`

---

**Predictions are not coming through (`Error sending request`)**

The prediction model is not running or is not reachable at the address set in `MODEL_URL`. Start the model service first, then restart this service.

---

**The service reconnects frequently (`[MQTT] Reconnected to broker`)**

Occasional reconnections are normal. If it is happening every few seconds, check:
- Network stability on the machine running the service
- Whether the HiveMQ broker is rejecting the connection (check for `MQTT error` lines above the reconnect message)

---

