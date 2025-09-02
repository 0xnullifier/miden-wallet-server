// signaling-server.js
import WebSocket from "ws";
const wss = new WebSocket.Server({ port: 8080 });

let clients: { ws: WebSocket, wallet: string }[] = [];

let walletClientMap: { [wallet: string]: WebSocket } = {};

console.log('Signaling server started on ws://localhost:8080');


wss.on('connection', (ws) => {
    clients.push({ ws: ws, wallet: '' });
    console.log('New client connected. Total clients:', clients.length);
    ws.on('message', (message) => {
        const jsonMessage = JSON.parse(message.toString());
        console.log('Received message:', jsonMessage);
        switch (jsonMessage.type) {
            case 'REGISTER':
                const client = clients.find(c => c.ws === ws);
                if (client) {
                    client.wallet = jsonMessage.wallet;
                    walletClientMap[jsonMessage.wallet] = ws;
                }
                break;
            case 'CREATE_OFFER':
                const targetClient = walletClientMap[jsonMessage.to];
                console.log('Target client for offer:', targetClient);
                const fromWallet = clients.find(c => c.ws === ws)!.wallet;
                if (targetClient !== undefined) {
                    targetClient.send(JSON.stringify({ type: "OFFER", 'offer': jsonMessage.offer, 'from': fromWallet }));
                } else {
                    console.log(`Client ${jsonMessage.to} is offline.`);
                    ws.send(JSON.stringify({ type: 'RECEIVER_OFFLINE', message: `Client ${jsonMessage.to} is offline. please ask them to open app` }));
                }
                break;
            case "FORWARD_ANSWER":
                const targetClientAnswer = walletClientMap[jsonMessage.to];
                const fromWalletAnswer = clients.find(c => c.ws === ws)!.wallet;
                if (targetClientAnswer) {
                    targetClientAnswer.send(JSON.stringify({ type: "ANSWER", 'answer': jsonMessage.answer, 'from': fromWalletAnswer }));
                    break;
                } else {
                    ws.send(JSON.stringify({ type: 'RECEIVER_OFFLINE', message: `Client ${jsonMessage.to} is offline. please ask them to open app` }));
                    break;
                }
            case "FORWARD_ICE_CANDIDATE":
                const targetClientIce = walletClientMap[jsonMessage.to];
                const fromWalletIce = clients.find(c => c.ws === ws)!.wallet;
                if (targetClientIce) {
                    targetClientIce.send(JSON.stringify({ type: "ICE_CANDIDATE", 'iceCandidate': jsonMessage.candidate, 'from': fromWalletIce }));
                } else {
                    ws.send(JSON.stringify({ type: 'RECEIVER_OFFLINE', message: `Client ${jsonMessage.to} is offline. please ask them to open app` }));
                }
                break;
            default:
                console.log('Unknown message type:', jsonMessage);
        }
    });
    ws.on('close', () => {
        clients = clients.filter(c => c.ws !== ws);
        console.log('Client disconnected. Remaining clients:', clients.length);
        Object.keys(walletClientMap).forEach(wallet => {
            if (walletClientMap[wallet] === ws) {
                delete walletClientMap[wallet];
            }
        });
    });
});
