CREATE TABLE IF NOT EXISTS scan_logs (
    id BIGSERIAL PRIMARY KEY,
    timestamp TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    ip_alvo INET,
    nivel_log TEXT NOT NULL,
    tipo_evento TEXT NOT NULL,
    mensagem_detalhada TEXT
);

CREATE INDEX IF NOT EXISTS idx_scan_logs_timestamp ON scan_logs(timestamp);
CREATE INDEX IF NOT EXISTS idx_scan_logs_tipo_evento ON scan_logs(tipo_evento);
CREATE INDEX IF NOT EXISTS idx_scan_logs_ip_alvo ON scan_logs(ip_alvo);
