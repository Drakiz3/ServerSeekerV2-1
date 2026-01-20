CREATE TABLE IF NOT EXISTS servers (
    address INET NOT NULL,
    port INTEGER NOT NULL,
    software TEXT,
    version TEXT,
    protocol INTEGER,
    icon TEXT,
    description_raw TEXT,
    description_formatted TEXT,
    prevents_chat_reports BOOLEAN,
    enforces_secure_chat BOOLEAN,
    first_seen BIGINT,
    last_seen BIGINT,
    online_players INTEGER,
    max_players INTEGER,
    country VARCHAR(2),
    asn TEXT,
    PRIMARY KEY (address, port)
);

CREATE TABLE IF NOT EXISTS players (
    address INET NOT NULL,
    port INTEGER NOT NULL,
    uuid UUID NOT NULL,
    name TEXT,
    first_seen BIGINT,
    last_seen BIGINT,
    PRIMARY KEY (address, port, uuid)
);

CREATE TABLE IF NOT EXISTS mods (
    address INET NOT NULL,
    port INTEGER NOT NULL,
    id TEXT NOT NULL,
    mod_marker TEXT,
    PRIMARY KEY (address, port, id)
);

CREATE TABLE IF NOT EXISTS countries (
    network CIDR,
    country VARCHAR(255),
    country_code VARCHAR(2),
    asn VARCHAR(16),
    company VARCHAR(255),
    PRIMARY KEY(network)
);

CREATE INDEX IF NOT EXISTS countries_table_index ON countries USING GIST (network inet_ops);
