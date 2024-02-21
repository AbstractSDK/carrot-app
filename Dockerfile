# Use the official Rust image as the base
FROM rust:latest

# Create a new empty shell project
RUN mkdir carrot_workspace
WORKDIR /carrot_workspace

# Copy your source tree
COPY ./bot ./bot
COPY ./contracts ./contracts
# Also copy Cargo.toml 
COPY Cargo.toml ./

# RUN echo | ls && exit 1

# Build your application
RUN cargo build --bin prod --release

# Install needed libraries for a Rust binary
# This might change based on your project's needs
RUN apt-get update && apt-get install -y libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*

# Copy the binary from the builder stage
RUN mv target/release/prod .

# Patch until https://linear.app/abstract-sdk/issue/ORC-79/fix-cw-orch-crashing-when-theres-no-state-file is fixed.
RUN mkdir ~/.cw-orchestrator
RUN echo "{}" > ~/.cw-orchestrator/state.json

# Command to run the binary
CMD ["./prod", "--fcd", "1h", "--acd", "1d"]
