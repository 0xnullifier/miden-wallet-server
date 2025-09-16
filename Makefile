CG=cargo
ND=node
SIGNALING_SERVER_DIR=signaling-server
PM=/root/.local/share/pnpm/pnpm
BIN=target/release/miden-faucet-server
BIN_MINT_SERVER=target/release/mint-server


install_deps_ec2:
	sudo yum update -y
	sudo yum install -y gcc gcc-c++ nodejs
	curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
	curl -fsSL https://get.pnpm.io/install.sh | sh -
	. ~/.bashrc
	pnpm env use --global 18

copy_services:
	sudo cp deployments/signaling.service /etc/systemd/system/signaling.service
	sudo cp deployments/server.service /etc/systemd/system/server.service
	sudo cp deployments/mint-server.service /etc/systemd/system/mint-server.service
	sudo systemctl daemon-reload
	sudo systemctl enable signaling
	sudo systemctl enable server
	sudo systemctl enable mint-server


build_rust:
	$(CG) build --release

build_node:
	cd $(SIGNALING_SERVER_DIR) && $(PM) install && $(PM) run build


build: build_rust build_node

start_signaling_server:
	cd $(SIGNALING_SERVER_DIR) && $(PM) run start

start_server:
	$(BIN) start-server

start_mint_server:
	$(BIN_MINT_SERVER)

start: start_signaling_server  start_server  start_mint_server