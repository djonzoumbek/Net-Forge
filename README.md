# NetForge

Client de terminal desktop personnel (SSH, puis Telnet et série) pour administrer
serveurs Linux, équipements Cisco/Huawei et laboratoires GNS3 / EVE-NG / PNETLab.
Cible : Ubuntu. Tout fonctionne en local, sans service cloud.

**État : phase 4 (Telnet et sessions multiples).** SSH et Telnet, une session par
onglet, profils rangés en dossiers, mots de passe dans le trousseau système,
`known_hosts`, mode compatibilité pour équipements anciens.

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

Raccourcis : `Ctrl+Shift+C` copier la sélection, `Ctrl+Shift+V` coller,
`Ctrl+Tab` / `Ctrl+Shift+Tab` (ou `Ctrl+PageDown` / `Ctrl+PageUp`) changer d'onglet,
clic molette sur un onglet pour le fermer.

## Telnet

Pour les consoles de laboratoire (GNS3, EVE-NG, PNETLab) et les équipements sans SSH.
**Tout circule en clair**, mots de passe compris : un badge « clair » / « Non chiffré »
le rappelle sur chaque session Telnet. Aucun mot de passe n'est enregistré pour un
profil Telnet (l'équipement le demande dans le terminal).

Négociation prise en charge : ECHO, SUPPRESS-GO-AHEAD, NAWS (taille de fenêtre,
transmise quand l'équipement l'accepte) et TERMINAL-TYPE (`XTERM-256COLOR`).
La touche Entrée envoie `CR NUL`, comme le client `telnet` standard (RFC 854).

## Données

Dans `~/.local/share/dev.kamis.netforge/` (droits `0700`) :

| Fichier | Contenu |
|---|---|
| `netforge.db` (+ `-wal`, `-shm`) | profils et dossiers (SQLite). **Aucun secret** : seulement le nom de l'entrée du trousseau |
| `known_hosts` | clés d'hôte acceptées dans NetForge (format OpenSSH, `0600`) |

- Mots de passe et passphrases : trousseau système (GNOME Keyring, service « NetForge »,
  entrées `profile-<id>`), visibles dans l'application « Mots de passe et clés ».
- `~/.ssh/known_hosts` est lu (jamais modifié) : un serveur déjà connu d'OpenSSH est
  reconnu. En cas de conflit, le fichier de NetForge prévaut.
- Migrations SQL versionnées : `src-tauri/migrations/` (version dans `PRAGMA user_version`).

## Vérifications

```bash
npm run typecheck && npm run lint                       # frontend
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings
cargo test                                              # tests unitaires
```

### Tests d'intégration SSH et Telnet (Docker)

Des serveurs jetables (Alpine, avec vim/htop/less/ping) écoutent sur `127.0.0.1`
uniquement : SSH port **2222** (moderne), SSH port **2223** (algorithmes legacy
seulement, comme un vieux Cisco IOS / Huawei VRP) et Telnet port **2323**
(busybox telnetd, avec invite `login:`).

```bash
./scripts/ssh-test-server.sh start        # utilisateur netforge / mot de passe netforge-test
cd src-tauri && cargo test -- --ignored
./scripts/ssh-test-server.sh rotate-keys  # nouvelles clés d'hôte (test « clé modifiée »)
./scripts/ssh-test-server.sh stop
```

`cargo test -- --ignored` inclut `system_keyring_round_trip`, qui écrit puis supprime une
entrée `netforge-test-entry` dans le vrai trousseau.

Les clés de test sont générées dans `src-tauri/target/ssh-test/` (ignoré par git) :
`id_plain` (sans passphrase) et `id_pass` (passphrase `test-passphrase`).
Le test `network_cut_is_detected_by_keepalive` met le conteneur en pause environ
une minute.

## Organisation

```
src/                      Frontend React + TypeScript
  features/terminal/      xterm.js, contrôle de flux de la sortie, file d'entrée
  features/sessions/      onglets (reducer), panneau d'une session
  features/connections/   connexion rapide, arborescence et éditeur de profils
  components/             barre d'onglets, barre d'état, dialogues
  services/               appels Tauri typés
  types/                  types partagés avec le backend
src-tauri/src/
  commands/               commandes Tauri (validation, couche fine)
  sessions/               gestionnaire de sessions et boucle par session
  transport/              trait commun, ssh/ (russh, algorithmes modernes/legacy),
                          telnet/ (négociation RFC 854/1073/1091), net.rs (TCP)
  persistence/            SQLite : migrations, profils, dossiers
  security/               trousseau (secrets), known_hosts
  errors/                 type d'erreur central { code, message }
src-tauri/migrations/     migrations SQL versionnées
scripts/                  serveur SSH de test
```

## Sécurité

- Algorithmes SSH modernes uniquement par défaut (y compris sans `ssh-rsa`/SHA-1).
  Le « mode compatibilité équipements anciens » d'un profil ajoute, après les modernes :
  `diffie-hellman-group1/14-sha1`, `diffie-hellman-group-exchange-sha1`, `ssh-rsa`,
  `aes*-cbc`, `3des-cbc`, `hmac-sha1`. Un badge « Legacy » signale leur utilisation.
- Première connexion : empreinte SHA256 affichée et confirmée, puis mémorisée.
  Clé modifiée : connexion bloquée, anciennes et nouvelles empreintes affichées,
  remplacement uniquement après confirmation explicite.
- Mots de passe et passphrases jamais journalisés, effacés de la mémoire côté Rust
  après usage (`zeroize`).
- Capabilities Tauri limitées aux commandes de NetForge ; CSP stricte en production.
