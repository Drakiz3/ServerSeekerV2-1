CREATE TABLE IF NOT EXISTS server_details (
    address INET NOT NULL,
    port INTEGER NOT NULL,
    plugins JSONB,
    world_info JSONB,
    detailed_version TEXT,
    auth_type TEXT,
    last_join_attempt BIGINT,
    join_success BOOLEAN DEFAULT FALSE,
    PRIMARY KEY (address, port),
    FOREIGN KEY (address, port) REFERENCES servers(address, port) ON DELETE CASCADE
);
