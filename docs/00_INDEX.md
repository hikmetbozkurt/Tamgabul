# Document Index and normative language

**MUST** = release-blocking requirement; **SHOULD** = preferred and needs documented exception; **MAY** = optional.

| ID | Belge | İçerik |
|---|---|---|
| 01 | [PRD](docs/01_PRODUCT_PRD.md) | Kullanıcı, problem, ürün ilkeleri, başarı |
| 02 | [MVP](docs/02_MVP_SCOPE.md) | Dahil/hariç, acceptance ve quality gates |
| 03 | [Architecture](docs/03_ARCHITECTURE.md) | Bileşenler, akış, thread/process boundaries |
| 04 | [Model registry](docs/04_MODEL_REGISTRY.md) | Model ekleme, uyumluluk, durum semantiği |
| 05 | [Lifecycle](docs/05_MODEL_LIFECYCLE.md) | Runtime yükleme/boşaltma, TTL, VRAM |
| 06 | [Memory](docs/06_MEMORY_ENGINE.md) | Session/episodic/semantic memory |
| 07 | [Indexing](docs/07_INDEXING_PERMISSIONS.md) | Scope, C:\, indeksleme, revoke |
| 08 | [Retrieval & dialogue](docs/08_RETRIEVAL_CONVERSATION.md) | Deterministic router + optional LLM |
| 09 | [UI/UX](docs/09_UI_UX.md) | Minimal widget, Settings, durum ikonları |
| 10 | [Platforms](docs/10_PLATFORM_WINDOWS_MACOS.md) | Windows tray/macOS menu bar ve yollar |
| 11 | [Security](docs/11_SECURITY_PRIVACY.md) | Threat model ve savunmalar |
| 12 | [Data schema](docs/12_DATA_SCHEMA.md) | SQLite, migrations, retention |
| 13 | [Contracts](docs/13_API_CONTRACTS.md) | Rust↔UI ve adapters, events |
| 14 | [Evals](docs/14_TEST_EVAL.md) | Benchmarks, QA, metrics |
| 15 | [Roadmap](docs/15_ROADMAP.md) | Milestones, epics, dependencies |
| 16 | [LoRA lab](docs/16_LORA_LAB.md) | Future RTX 4080 experiment template |
| 17 | [Distribution](docs/17_DISTRIBUTION_OPS.md) | Install, package, update, failures |
| 18 | [Risk register](docs/18_RISK_REGISTER.md) | Technical, product & legal uncertainties |
| 19 | [Research](docs/19_RESEARCH_VERIFICATION.md) | Evidence links & unverified claims |
| 20 | [ADRs](docs/20_ADR.md) | Ratified architecture decisions |
| 21 | [User journeys](docs/21_USER_JOURNEYS.md) | Example flows including past-reference |
| 22 | [Platform matrix](docs/22_CROSS_PLATFORM_MATRIX.md) | Compatibility test matrix |
| 23 | [Agent handoff](docs/23_AGENT_HANDOFF.md) | Parallel engineering workflow |

`database/001_initial.sql`: proposed schema, reviewed with SQLite syntax smoke test. `examples/`: config, model manifest, intent schema, evaluation fixture. `agent_tasks/`: first concrete agent tickets. `templates/`: reusable task, ADR and experiment reports. `ai_lab/`: future training protocol. All thresholds initially **targets to validate**, not observed measurements.
