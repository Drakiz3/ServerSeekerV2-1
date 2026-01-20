const express = require('express');
const bodyParser = require('body-parser');
const mineflayer = require('mineflayer');

const app = express();
app.use(bodyParser.json());

const PORT = process.env.PORT || 3000;

app.post('/join', async (req, res) => {
    const { host, port, version } = req.body;

    if (!host || !port) {
        return res.status(400).json({ error: 'Host and port are required' });
    }

    try {
        const result = await scanServer(host, parseInt(port), version);
        res.json(result);
    } catch (e) {
        res.status(500).json({ error: e.message });
    }
});

async function scanServer(host, port, version) {
    return new Promise((resolve) => {
        const username = 'Seeker' + Math.floor(Math.random() * 10000);
        let plugins = [];
        let chatLog = [];
        let ended = false;

        // Timeout de segurança global para a promessa
        const timeout = setTimeout(() => {
            if (!ended) {
                finish('timeout');
            }
        }, 15000);

        const botOptions = {
            host: host,
            port: port,
            username: username,
            auth: 'offline',
            hideErrors: true,
            checkTimeoutInterval: 10000
        };

        if (version) {
            botOptions.version = version;
        }

        let bot;

        function finish(status, extra = {}) {
            if (ended) return;
            ended = true;
            clearTimeout(timeout);

            // Tentar extrair versão do bot se disponível
            let detectedVersion = version;
            if (bot && bot.version) detectedVersion = bot.version;

            try {
                if (bot) bot.quit();
            } catch (e) { }

            resolve({
                status,
                online: status === 'success',
                version: detectedVersion,
                plugins: [...new Set(plugins)], // unique
                chat: chatLog,
                ...extra
            });
        }

        // Pre-login Delay: 500ms to 2500ms
        const preLoginDelay = Math.floor(Math.random() * 2000) + 500;

        setTimeout(() => {
            if (ended) return;

            try {
                bot = mineflayer.createBot(botOptions);
            } catch (err) {
                finish('error', { error: err.message });
                return;
            }

            bot.on('login', () => {
                // Command Jitter: 0 to 500ms extra delay
                const jitter = () => Math.floor(Math.random() * 500);

                // Tentar comandos básicos com jitter
                setTimeout(() => { if (!ended) bot.chat('/plugins'); }, 500 + jitter());
                setTimeout(() => { if (!ended) bot.chat('/version'); }, 1000 + jitter());
                setTimeout(() => { if (!ended) bot.chat('/pl'); }, 1500 + jitter());

                // Finalizar após alguns segundos de coleta
                setTimeout(() => {
                    finish('success');
                }, 3000 + jitter());
            });

            bot.on('message', (jsonMsg) => {
                const plainText = jsonMsg.toString();
                if (!plainText) return;

                chatLog.push(plainText);

                // Tentar parsear plugins (formato padrão Bukkit/Spigot)
                // "Plugins (3): WorldEdit, Essentials, LogBlock"
                const pluginMatch = plainText.match(/Plugins? \(\d+\): (.+)/i);
                if (pluginMatch && pluginMatch[1]) {
                    const found = pluginMatch[1].split(',').map(s => s.trim().split(' v')[0]);
                    plugins.push(...found);
                }

                // Formato alternativo: "Plugins: a, b, c"
                if (plainText.startsWith("Plugins:")) {
                    const list = plainText.substring(8).split(',').map(s => s.trim());
                    plugins.push(...list);
                }
            });

            bot.on('kicked', (reason) => {
                let reasonText = reason;
                try {
                    const r = JSON.parse(reason);
                    reasonText = r.text || JSON.stringify(r);
                } catch (e) { }
                finish('kicked', { reason: reasonText });
            });

            bot.on('error', (err) => {
                if (!ended) finish('error', { error: err.message });
            });

            bot.on('end', () => {
                if (!ended) finish('ended');
            });
        }, preLoginDelay);
    });
}

app.listen(PORT, () => {
    console.log(`Scanner bot listening on port ${PORT}`);
});
