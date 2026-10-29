# NetForge

Client de terminal desktop personnel (SSH, puis Telnet et série) pour administrer
serveurs Linux, équipements Cisco/Huawei et laboratoires GNS3 / EVE-NG / PNETLab.
Cible : Ubuntu. Tout fonctionne en local, sans service cloud.

**État : phase 2 (prototype SSH interactif).** Une session SSH à la fois, sans
profils enregistrés ; les clés d'hôte sont confirmées à chaque connexion
(mémorisation `known_hosts` en phase 3).

## Prérequis (Ubuntu)

```bash
sudo apt install build-essential curl wget file pkg-config libwebkit2gtk-4.1-dev \
  libssl-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libudev-dev libdbus-1-dev
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
rustup component add clippy rustfmt
```

Node.js ≥ 20 et npm sont aussi nécessaires.

## Démarrage

```bash
npm install
npm run tauri dev        # application en mode développement
```

Logs techniques : sortie standard, niveau réglable avec `RUST_LOG`
(ex. `RUST_LOG=netforge_lib=debug npm run tauri dev`). Ils ne contiennent ni
mots de passe ni contenu des sessions.

Raccourcis du terminal : `Ctrl+Shift+C` copier la sélection, `Ctrl+Shift+V` coller.

## Vérifications

```bash
npm run typecheck && npm run lint                       # frontend
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings
cargo test                                              # tests unitaires
```

### Tests d'intégration SSH (Docker)

Un serveur OpenSSH jetable (Alpine, avec vim/htop/less/ping) écoute sur
`127.0.0.1:2222` uniquement :

```bash
./scripts/ssh-test-server.sh start     # utilisateur netforge / mot de passe netforge-test
cd src-tauri && cargo test -- --ignored
./scripts/ssh-test-server.sh stop
```

Les clés de test sont générées dans `src-tauri/target/ssh-test/` (ignoré par git) :
`id_plain` (sans passphrase) et `id_pass` (passphrase `test-passphrase`).
Le test `network_cut_is_detected_by_keepalive` met le conteneur en pause environ
une minute.

## Organisation

```
src/                      Frontend React + TypeScript
  features/terminal/      xterm.js, contrôle de flux de la sortie, file d'entrée
  features/connections/   formulaire de connexion
  components/             barre d'état, dialogue de clé d'hôte
  services/               appels Tauri typés
  types/                  types partagés avec le backend
src-tauri/src/
  commands/               commandes Tauri (validation, couche fine)
  sessions/               gestionnaire de sessions et boucle par session
  transport/              trait commun + ssh/ (russh)
  errors/                 type d'erreur central { code, message }
scripts/                  serveur SSH de test
```

## Sécurité

- Algorithmes SSH modernes uniquement par défaut (mode compatibilité legacy : phase 3).
- Empreinte SHA256 de la clé d'hôte affichée et confirmée avant toute authentification.
- Mots de passe et passphrases jamais journalisés, effacés de la mémoire côté Rust
  après usage (`zeroize`).
- Capabilities Tauri limitées aux commandes de session ; CSP stricte en production.
