# Symphony Automation Service

Symphony is a long-running automation service. The backend is built in Rust.

## Project Setup

The Rust backend is located in the `backend` directory.
It is initialized as a standard Rust binary project and uses the following core dependencies:
- **tokio**: Async runtime
- **serde**: Serialization / Deserialization
- **chrono**: Date and time handling

## Domain Models

The core domain models are defined in `backend/src/domain/models.rs`. The structs implemented include:
- `Issue`: Represents a task/issue with fields like id, title, state, and labels.
- `Workspace`: Tracks the workspace for agent execution.
- `RunAttempt`: Represents the state of an agent execution attempt.
- `LiveSession`: Tracks active subprocess sessions.
- `RetryEntry`: Tracks retry scheduling for failed attempts.

## Running Checks and Tests

To verify that the code compiles correctly, run:

```bash
cd backend
cargo check
```

To run the unit tests (which include instantiation tests for the domain models), run:

```bash
cd backend
cargo test
```
