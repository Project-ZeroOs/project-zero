# Project Zero: Personal Compute Fabric & Distributed Execution

## 1. Overview & Research Scope

The **Personal Compute Fabric** is the distributed computation and resource-sharing subsystem of Project Zero. 

Its objective is to eliminate the physical boundaries between a user's devices by enabling transparent pooling of:
* **CPU cores** (compilation, data processing, background tasks)
* **GPU compute & rendering** (parallel compute, ray tracing, video encoding, interactive rendering)
* **NPU / Accelerators** (neural network inference, embedding generation, semantic search)
* **RAM / Virtual Memory** (distributed caching, large-model execution)
* **Sensors & Connectivity** (GPS, 5G modem, camera, hardware accelerators)

```text
                               +-----------------------------+
                               |     USER WORKLOAD TASK      |
                               +--------------+--------------+
                                              |
                               +--------------v--------------+
                               |      WORKLOAD ANALYZER      |
                               +--------------+--------------+
                                              |
                               +--------------v--------------+
                               |  DYNAMIC COMPUTE PLANNER    |
                               | (Multi-Variable Cost Func)  |
                               +--------------+--------------+
                                              |
                     +------------------------+------------------------+
                     |                                                 |
           +---------v---------+                             +---------v---------+
           |    LOCAL NODE     |                             |   REMOTE PEER     |
           | (Smartphone/Host) |                             | (Laptop/Workstn)  |
           +-------------------+                             +-------------------+
```

---

## 2. The Physics of Distributed Computing: Confronting Network Realities

A fatal error in distributed systems design is the assumption of transparent, instantaneous network offloading. In the real world, distributed execution is constrained by immutable physical laws:

### 2.1 The Speed of Light and Latency Lower Bounds
Light travels through vacuum at $\approx 300\text{ km/ms}$ and through optical fiber at $\approx 200\text{ km/ms}$ ($5\text{ }\mu\text{s/km}$).
* A round-trip time (RTT) over a $500\text{ km}$ fiber distance cannot physically fall below:
  $$\text{RTT}_{\text{min}} = \frac{2 \times 500\text{ km}}{200\text{ km/ms}} = 5.0\text{ ms}$$
* When accounting for router queuing, bufferbloat, serialization delay, and Wi-Fi airtime contention, real-world WAN RTT between distant cities routinely exceeds $30 - 80\text{ ms}$.

### 2.2 Bandwidth-Delay Product (BDP) & Serialization
Transferring large state across the network incurs non-trivial transmission delays:
$$\text{Serialization Time} = \frac{\text{Data Size (bits)}}{\text{Available Bandwidth (bps)}}$$
* Offloading a $1\text{ GB}$ memory snapshot over a $100\text{ Mbps}$ link takes at minimum $80\text{ seconds}$.
* Even over a $10\text{ Gbps}$ local Thunderbolt/Wi-Fi 7 bridge, moving $1\text{ GB}$ takes $\approx 800\text{ ms}$.
* Therefore: **Data locality and incremental state delta transmission are paramount**.

---

## 3. Strict Taxonomy of Distributed Offloading

Project Zero rigorously distinguishes between three fundamentally different offloading classes:

```text
1. REMOTE COMPUTATION (Stateless / Packaged Batch)
   [Client] ───(Task Spec + Input Data)───> [Remote Node]
   [Client] <──────(Result Payload)──────── [Remote Node]

2. REMOTE RENDERING (Interactive Display Stream)
   [Client] ──────(Input Events)──────────> [Remote GPU]
   [Client] <──(Low-Latency Video Stream)── [Remote Hardware Encoder]

3. COLLABORATIVE COMPUTATION (Shared Parallel Fabric)
   [Client GPU] <───(Synchronized State / Delta)───> [Remote GPU]
        │                                                  │
        ▼                                                  ▼
   [Local Frame Half / Tiles]                      [Remote Frame Half / Tiles]
```

### 3.1 Remote Computation (RPC / Batch Compute)
* **Characteristics**: Input data is sent once; the remote node computes asynchronously; results are returned.
* **Latency Sensitivity**: Low to Moderate ($> 100\text{ ms}$ acceptable).
* **Ideal Use Cases**: Code compilation (`gcc`, `rustc`), deep learning model inference (LLM prompt processing), media transcode, background search indexing.
* **Feasibility**: High across both LAN and WAN.

### 3.2 Remote Rendering (Video Streaming)
* **Characteristics**: The remote GPU renders the entire visual scene into a framebuffer, feeds it directly to a hardware video encoder (AV1 / HEVC NVENC), and streams compressed frames via RTP/QUIC to the local client for hardware decode and scanout.
* **Latency Sensitivity**: Extremely High (Round-trip budget $< 16\text{ ms}$ for 60 fps interactivity).
* **Constraint**: Over a WAN with $> 30\text{ ms}$ RTT, remote rendering suffers from noticeable input lag (rubber-banding). Mitigated using local input prediction and asynchronous client-side cursor reprojection.

### 3.3 Collaborative Rendering (Split-Frame / Tile Partitioning)
* **Characteristics**: Local and remote GPUs cooperatively render parts of the same frame.
* **The Reality Check**: Sharing geometry, textures, and synchronization barriers across a network link introduces immense bus latency. PCIe Gen 4 offers $32\text{ GB/s}$ at $< 1\text{ }\mu\text{s}$ latency. A $10\text{ Gbps}$ network link offers only $1.25\text{ GB/s}$ at $> 100\text{ }\mu\text{s}$ latency.
* **Scientific Conclusion**: Unbounded distributed SLI across network links is physically infeasible for real-time rasterization. Collaborative rendering is viable **only** for embarrassingly parallel workloads, such as path tracing (where tiles require zero inter-thread communication during bounce evaluation) or offline baking.

---

## 4. The Latency-Aware Compute Planner

The **Compute Planner** is an autonomous scheduler that continuously computes the optimal execution target for every schedulable task.

### 4.1 Multi-Variable Cost Function
For a given task $T$ with input size $D_{\text{in}}$, output size $D_{\text{out}}$, and compute complexity $W$ (FLOPs), the planner evaluates the estimated cost $C_i$ for each candidate node $i$ (including local host $0$):

$$C_i = w_T \cdot T_i + w_E \cdot E_i + w_P \cdot P_i$$

Where:
* $T_i$ is Total Latency:
  $$T_i = \text{RTT}_i + \frac{D_{\text{in}}}{\text{BW}_{\text{up}, i}} + \frac{W}{\text{ComputePower}_i} + \frac{D_{\text{out}}}{\text{BW}_{\text{down}, i}}$$
* $E_i$ is Local Energy Impact (battery drain penalty on mobile host).
* $P_i$ is Privacy & Data Sovereignty Cost (0 if data is allowed to leave device; $\infty$ if restricted by capability policy).
* $w_T, w_E, w_P$ are dynamic weights parameterized by user power state and current intent.

### 4.2 Latency Threshold Guidelines

| Latency Budget | Permitted Execution Target | Workload Types |
| :--- | :--- | :--- |
| **$< 10\text{ ms}$** | **Strictly Local Only** | Direct touch/pointer handling, keyboard echo, audio synthesis, compositor scanout |
| **$10 - 50\text{ ms}$** | **Local or Ultra-Low-Latency LAN** | Fast autocomplete, physics simulation, local tile rendering, UI state transitions |
| **$50 - 200\text{ ms}$** | **LAN or Nearby Edge Node** | Medium compilation units, semantic search queries, document summarization |
| **$> 200\text{ ms}$** | **Unconstrained Remote / Fabric Node** | Full workspace build, model fine-tuning, large media export, bulk storage replication |

---

## 5. Empirical Benchmark & Experimentation Protocol

To adhere to Project Zero's scientific methodology, all distributed fabric features must pass the **Fabric Benchmark Suite (FBS)**:

1. **Synthetic Telemetry Probe**:
   * Measures continuous RTT, jitter, packet loss distribution, and available bandwidth over varying network topologies (Direct Thunderbolt, Wi-Fi 6E/7, 5G cellular, WAN relay).
2. **Compute-to-Payload Ratio Sweep**:
   * Evaluates the break-even curve: at what computational intensity (FLOPs/byte) does remote offloading outweigh serialization overhead?
3. **Thermal & Battery Profiling**:
   * Compares the milliamp-hour (mAh) consumption of local smartphone CPU execution vs. Wi-Fi transceiver transmission to determine net battery savings.
