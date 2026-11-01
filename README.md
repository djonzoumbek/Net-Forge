# NetForge

Client de terminal desktop personnel (SSH, puis Telnet et série) pour administrer
serveurs Linux, équipements Cisco/Huawei et laboratoires GNS3 / EVE-NG / PNETLab.
Cibles : Ubuntu (principale), Arch Linux et Windows 10/11. Tout fonctionne en local,
sans service cloud.

**État : phases 1 à 8 terminées, portage Arch et Windows en cours de validation.** SSH, Telnet et console série, une session
par onglet, profils rangés en dossiers, mots de passe dans le trousseau système,
`known_hosts`, mode compatibilité pour équipements anciens, keepalive et reconnexion
automatique, barre de commandes, envoi multi-sessions, gestion du pager, journal de
session, recherche dans l'historique, thèmes, polices, raccourcis clavier, transfert
de fichiers SFTP, hôte de rebond (ProxyJump), redirections de port (tunnels -L et SOCKS), copie à la sélection et collage au clic droit. Paquets : `.deb` et
AppImage (Ubuntu), `PKGBUILD` (Arch), installateurs `.exe` / `.msi` (Windows, construits
par GitHub Actions).

## Installation (paquet .deb)

```bash
sudo apt install ./NetForge_0.1.0_amd64.deb
```

`apt` (et non `dpkg -i`) installe aussi les dépendances (WebKitGTK, GTK 3, libudev,
libdbus). NetForge apparaît ensuite dans le menu des applications (catégorie
Réseau), ou se lance avec la commande `netforge`.

Fichiers installés : `/usr/bin/netforge`, `/usr/share/applications/NetForge.desktop`
et les icônes dans `/usr/share/icons/hicolor/*/apps/netforge.png`. Aucun service,
aucun fichier hors de ces chemins.

Mise à jour : réinstaller un `.deb` plus récent de la même façon. Les profils sont
conservés ; une base d'une version précédente est migrée automatiquement, après une
copie de sauvegarde (voir [Données](#données)).

### Désinstallation

```bash
sudo apt remove net-forge        # retire le programme
```

Comme pour toute application, tes données personnelles restent en place (elles sont
dans ton dossier personnel, pas dans le paquet). Pour tout effacer :

```bash
rm -rf ~/.local/share/dev.kamis.netforge     # profils, known_hosts, cache
rm -rf ~/Documents/NetForge/logs             # journaux de session (si utilisés)
```

et supprimer les entrées « NetForge » dans l'application « Mots de passe et clés »
(trousseau GNOME).

### AppImage (sans installation)

Un seul fichier exécutable, utile pour essayer NetForge sans droits administrateur :

```bash
chmod +x NetForge_0.1.0_amd64.AppImage
./NetForge_0.1.0_amd64.AppImage
```

Il nécessite FUSE 2 (`sudo apt install libfuse2t64` s'il manque) ; sinon :
`./NetForge_0.1.0_amd64.AppImage --appimage-extract-and-run`. Il embarque WebKitGTK
(~87 Mo, contre ~6 Mo pour le `.deb`) et utilise les mêmes données que le `.deb`.
Pas d'entrée dans le menu des applications. Le `.deb` reste le format recommandé.

### Arch Linux

Le `PKGBUILD` compile NetForge depuis les sources d'une version publiée (tag `v<version>`
sur GitHub) et utilise les bibliothèques d'Arch (`webkit2gtk-4.1`, `systemd-libs`, `dbus`) :

```bash
cd packaging/arch
makepkg -si            # --nocheck pour sauter les tests (compilation plus courte)
```

Désinstallation : `sudo pacman -R netforge`. Pour enregistrer les mots de passe, il
faut un trousseau Secret Service (`gnome-keyring`, ou KWallet sous KDE). L'AppImage
fonctionne aussi sur Arch.

### Windows 10 / 11

Télécharger `NetForge_<version>_x64-setup.exe` (ou le `.msi`) depuis la page *Releases*
du dépôt GitHub, puis l'exécuter. WebView2 est déjà présent sur Windows 10/11 à jour ;
sinon l'installateur le télécharge.

Différences avec Linux :
- mots de passe dans le **Gestionnaire d'identifiants** Windows ;
- agent SSH : le service « OpenSSH Authentication Agent » (`ssh-add` de Windows), à
  défaut Pageant (PuTTY) ;
- ports série `COM3`, `COM4`… (Gestionnaire de périphériques → Ports (COM et LPT)) ;
- données dans `%APPDATA%\dev.kamis.netforge\`.

### Construire les installateurs (GitHub Actions)

- `.github/workflows/ci.yml` : à chaque push, typecheck, lint et tests du frontend,
  `cargo fmt`, `clippy` et tests Rust sur Ubuntu **et** Windows.
- `.github/workflows/release.yml` : pour un tag `v*`, construit `.deb` + AppImage
  (Ubuntu 24.04) et `.exe` + `.msi` (Windows), et les joint à une *release* en
  brouillon, à vérifier puis publier à la main :

  ```bash
  git tag v0.2.0 && git push origin v0.2.0
  ```

  Lancé à la main (onglet *Actions* → *Release* → *Run workflow*), il dépose les
  installateurs comme artefacts de l'exécution, sans créer de release.

## Compiler depuis les sources

Prérequis ci-dessous, puis :

```bash
npm install
npm run tauri build -- --bundles deb
# -> src-tauri/target/release/bundle/deb/NetForge_0.1.0_amd64.deb
npm run tauri build -- --bundles appimage
# -> src-tauri/target/release/bundle/appimage/NetForge_0.1.0_amd64.AppImage
```

La première construction de l'AppImage télécharge les outils `linuxdeploy` (~35 Mo,
gardés dans `~/.cache/tauri`).

La compilation release (optimisation complète, LTO) prend 5 à 10 minutes sur un
portable. Le binaire seul est dans `src-tauri/target/release/netforge`.

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

## Raccourcis clavier

| Raccourci | Action |
|---|---|
| `Ctrl+Shift+T` | nouvelle connexion |
| `Ctrl+Shift+D` | dupliquer l'onglet (même cible) |
| `Ctrl+Shift+W` (ou clic molette sur l'onglet) | fermer l'onglet |
| `Ctrl+Tab` / `Ctrl+Shift+Tab`, `Ctrl+PageDown` / `Ctrl+PageUp` | onglet suivant / précédent |
| `Ctrl+Shift+F` | rechercher dans l'historique (Entrée / Maj+Entrée, Échap pour fermer) |
| `Ctrl+Shift+C` / `Ctrl+Shift+V` | copier / coller |
| sélection à la souris / clic droit / `Ctrl+V` | copier / coller (désactivables dans Réglages → Presse-papiers) |
| `Ctrl++` / `Ctrl+-` / `Ctrl+0` | agrandir / réduire / taille normale du texte (non enregistré) |
| `Ctrl+Shift+B` | afficher / masquer les profils |
| `Ctrl+,` | réglages |

Ces raccourcis ne sont jamais envoyés au terminal distant ; `Ctrl+C`, `Ctrl+D`,
`Ctrl+Z`, `Ctrl+R`, `Ctrl+W`… restent au shell. Les raccourcis d'onglet, de recherche
et de zoom agissent sur la session active : sans session ouverte, ils sont sans effet.

**Presse-papiers** (Réglages, tout activé par défaut) : le texte sélectionné à la souris
est copié aussitôt, le clic droit colle, `Ctrl+V` colle. Désactiver `Ctrl+V` pour
envoyer `^V` au terminal distant (bloc visuel dans vim). Un collage de plusieurs
lignes demande toujours confirmation.

**Glisser-déposer** : glisser un onglet pour changer l'ordre des sessions ; glisser un
profil ou un dossier sur un dossier pour l'y ranger (ou dans l'espace vide de la liste
pour le remettre à la racine). `Échap` annule. Un dossier ne peut pas être déposé dans
un de ses propres sous-dossiers.

## Réglages et journal de session

- **Réglages** (`⚙` dans la barre des profils ou `Ctrl+,`) : thème du terminal
  (NetForge sombre/clair, Solarized sombre, contraste élevé), police et taille, taille
  de l'historique (1 000 à 200 000 lignes par onglet, 10 000 par défaut), dossier des
  journaux. Appliqués immédiatement aux onglets ouverts.
- **Journal de session** (option par profil, désactivé par défaut) : un fichier
  horodaté `<profil>_<date>.log` par connexion, dans `~/Documents/NetForge/logs/`
  (ou le dossier choisi), droits `0600`. Deux formats :
  - *texte brut* : séquences d'échappement, couleurs et invites du pager effacées,
    lisible dans n'importe quel éditeur ;
  - *octets bruts* : exactement ce qui a été reçu, rejouable avec `cat`.

  ⚠ Le journal contient tout ce que l'équipement affiche, donc parfois des secrets
  (configurations, saisies renvoyées en écho). Un badge « ● Journal » le signale.
  L'écriture se fait dans un thread dédié : si le disque ne suit pas, la session
  n'est jamais ralentie (les octets perdus sont signalés dans le fichier).

## Administration réseau

- **Keepalive** (par profil, 30 s par défaut, 0 = désactivé) : keepalive SSH ou
  `IAC NOP` Telnet, plus le keepalive TCP du noyau. Garde les sessions inactives
  ouvertes à travers pare-feu et NAT, et détecte une connexion coupée.
- **Reconnexion** : un clic dans le bandeau après une déconnexion. Option par profil :
  reconnexion automatique (2, 5, 10, 20 puis 30 s, 5 tentatives) **uniquement** si la
  connexion est perdue, jamais après un `exit`.
- **Barre de commandes** : boutons enregistrés envoyant une ou plusieurs lignes, avec
  un délai entre les lignes. Clic droit sur un bouton pour le modifier.
- **Diffuser…** : envoie les mêmes commandes à plusieurs sessions connectées, après
  une étape de vérification explicite.
- **Collage** : au-delà d'une ligne, une confirmation avec aperçu est demandée.
- **Pager** (par profil) : réponse automatique à `--More--`, `---- More ----`,
  `-- More --`, `<--- More --->`, `---(more)---` ; ou commandes envoyées à la
  première invite (préréglages `terminal length 0` / `screen-length 0 temporary`).
  Ces commandes sont stockées en clair : jamais de mot de passe dedans.

## Telnet

Pour les consoles de laboratoire (GNS3, EVE-NG, PNETLab) et les équipements sans SSH.
**Tout circule en clair**, mots de passe compris : un badge « clair » / « Non chiffré »
le rappelle sur chaque session Telnet. Aucun mot de passe n'est enregistré pour un
profil Telnet (l'équipement le demande dans le terminal).

Négociation prise en charge : ECHO, SUPPRESS-GO-AHEAD, NAWS (taille de fenêtre,
transmise quand l'équipement l'accepte) et TERMINAL-TYPE (`XTERM-256COLOR`).
La touche Entrée envoie `CR NUL`, comme le client `telnet` standard (RFC 854).

## Transfert de fichiers (SFTP)

Bouton **Fichiers** d'un onglet SSH connecté : un panneau s'ouvre sous le terminal, sur
**la même connexion** (pas de nouveau mot de passe ni de nouvelle vérification de clé).

- Navigation : double-clic sur un dossier, `↑` pour le parent, chemin modifiable.
- **Envoyer…** : un ou plusieurs fichiers vers le dossier affiché. Un fichier du même
  nom n'est jamais écrasé sans clic sur « Remplacer ».
- `↓` : télécharger (dossier Téléchargements proposé). Le fichier est écrit sous
  `<nom>.netforge-part` puis renommé une fois complet, avec les droits `0600`
  (une configuration contient souvent des secrets).
- Progression, annulation ; un transfert annulé ou en échec ne laisse aucun fichier
  partiel (ni local, ni distant).
- Fermer le panneau ne coupe pas les transferts ; fermer la session, si.
- Les dossiers entiers ne sont pas transférés (fichier par fichier). Beaucoup
  d'équipements réseau n'ont pas SFTP (seulement SCP) : un message le signale.

## Hôte de rebond (ProxyJump)

Comme `ssh -J` : un profil SSH peut être joint **à travers un autre profil SSH** (un
bastion). Dans le profil, choisir « Hôte de rebond (bastion) » ; l'hôte et le port du
profil sont alors ceux **vus depuis le bastion** (ex. `10.0.0.1:22`, une adresse privée
du labo).

- Le bastion utilise son propre profil : authentification, mot de passe du trousseau,
  mode compatibilité legacy, keepalive.
- Chaînes possibles (le bastion a lui-même un rebond), 3 sauts au plus ; les boucles
  sont refusées à l'enregistrement. Supprimer le profil bastion rend les autres directs.
- **Chaque clé d'hôte est vérifiée** (bastion puis cible), avec la même confirmation
  d'empreinte et le même blocage en cas de clé modifiée qu'en connexion directe.
- La barre d'état affiche « via <bastion> » (survol : toute la chaîne).
- Le mot de passe du bastion doit être enregistré dans son profil (ou utiliser une clé
  ou l'agent SSH) : il ne peut pas être saisi à la connexion.
- Erreurs propres au rebond : « Hôte de rebond … n'a pas pu joindre 10.0.0.1:22 »
  (cible injoignable depuis le bastion, ou `AllowTcpForwarding no` sur le bastion).

## Redirections de port (tunnels SSH)

Comme `ssh -L` et `ssh -D`, sur la connexion de la session (aucune nouvelle
authentification) :

- **Local (-L)** : un port de cette machine mène à une destination *vue depuis le
  serveur SSH*. Exemple : profil « bastion du labo », `8443 → 192.168.10.1:443`, puis
  `https://localhost:8443` dans le navigateur ouvre l'interface web du routeur.
- **SOCKS (-D)** : un proxy SOCKS5 (port 1080 par défaut). Configurer le navigateur sur
  `127.0.0.1:1080` avec la résolution DNS distante (`socks5h://` pour curl, « Proxy DNS
  via SOCKS v5 » dans Firefox) : tout le trafic web passe par le serveur.

Deux façons de les créer :
- dans le profil (section **Redirections de port**, SSH uniquement) : démarrées à
  chaque connexion, y compris après une reconnexion ;
- pendant la session : bouton **Tunnels** de la barre d'état, puis « Démarrer ».

Le panneau **Tunnels** liste les redirections actives avec l'adresse à utiliser, les
connexions en cours / depuis le démarrage, la dernière erreur (destination
injoignable, redirections interdites par le serveur — `AllowTcpForwarding no`, fréquent
sur les routeurs) et un bouton « Arrêter ». Si une redirection du profil ne démarre pas
(port déjà utilisé…), le bouton devient « ⚠ Tunnels » et le panneau affiche l'erreur.

Sécurité : l'écoute se fait sur `127.0.0.1` par défaut (cette machine seulement).
« Accessible depuis le réseau local » écoute sur `0.0.0.0` : n'importe quelle machine du
réseau peut alors utiliser le tunnel sans authentification, comme `ssh -g`. Comme avec
OpenSSH, le proxy SOCKS n'a pas de mot de passe : tout programme local peut s'en servir.
Fermer la session arrête ses redirections et libère les ports.

Non géré : la redirection distante (`ssh -R`).

## Console série

Protocole **Série** dans la connexion rapide ou un profil : port (ports détectés
listés, adaptateurs USB en premier), vitesse, bits de données, parité, bits de stop,
contrôle de flux. Par défaut 9600 8N1 sans contrôle de flux (consoles Cisco et Huawei).

**Droits d'accès** : sous Ubuntu, `/dev/ttyUSB*`, `/dev/ttyACM*` et `/dev/ttyS*`
appartiennent au groupe `dialout`. Une seule fois :

```bash
sudo usermod -aG dialout $USER     # puis se déconnecter et se reconnecter
```

NetForge détecte le problème (port marqué « accès refusé » dans la liste) et explique
la marche à suivre, y compris le cas « ajouté au groupe mais session pas encore
rouverte ». Un port déjà ouvert ailleurs (minicom, screen) est signalé comme tel ;
débrancher l'adaptateur est détecté comme une connexion perdue.

## Données

Dans `~/.local/share/dev.kamis.netforge/` (droits `0700`) sous Linux,
`%APPDATA%\dev.kamis.netforge\` sous Windows :

| Fichier | Contenu |
|---|---|
| `netforge.db` (+ `-wal`, `-shm`) | profils et dossiers (SQLite). **Aucun secret** : seulement le nom de l'entrée du trousseau |
| `known_hosts` | clés d'hôte acceptées dans NetForge (format OpenSSH, `0600`) |

Les redirections de port d'un profil sont dans la table `port_forwards` (migration 0007).

Journaux de session (si activés) : `~/Documents/NetForge/logs/` par défaut.

- Mots de passe et passphrases : trousseau système (GNOME Keyring, service « NetForge »,
  entrées `profile-<id>`), visibles dans l'application « Mots de passe et clés ».
- `~/.ssh/known_hosts` est lu (jamais modifié) : un serveur déjà connu d'OpenSSH est
  reconnu. En cas de conflit, le fichier de NetForge prévaut.
- Migrations SQL versionnées : `src-tauri/migrations/` (version dans `PRAGMA user_version`).
  Avant de migrer une base existante, NetForge en fait une copie
  `netforge.db.backup-v<N>` (état avant la migration depuis la version N).
- Pour essayer l'application sans toucher à ses vraies données :
  `XDG_DATA_HOME=/tmp/netforge-essai npm run tauri dev`.

## Vérifications

```bash
npm run typecheck && npm run lint && npm test          # frontend (Vitest)
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
cd src-tauri && cargo test -- --ignored   # voir la remarque ci-dessous
./scripts/ssh-test-server.sh rotate-keys  # nouvelles clés d'hôte (test « clé modifiée »)
./scripts/ssh-test-server.sh stop
```

`cargo test -- --ignored` inclut `system_keyring_round_trip`, qui écrit puis supprime une
entrée `netforge-test-entry` dans le vrai trousseau.

Les clés de test sont générées dans `src-tauri/target/ssh-test/` (ignoré par git) :
`id_plain` (sans passphrase) et `id_pass` (passphrase `test-passphrase`).
Le test `network_cut_is_detected_by_keepalive` met le conteneur en pause environ une
minute : lancé en même temps que les autres, il les fait échouer. Lance-le à part :

```bash
cargo test -- --include-ignored --skip network_cut_is_detected_by_keepalive --skip denied_port
cargo test -- --ignored network_cut_is_detected_by_keepalive
```

Les tests SFTP (`transfers::tests`) utilisent le même serveur Docker. Les tests du port
série n'ont besoin d'aucun matériel : ils utilisent une paire de pseudo-terminaux. Le
test `denied_port_explains_the_dialout_group` vérifie le message sur un vrai port
inaccessible (compte hors du groupe `dialout`) :
`NETFORGE_SERIAL_DENIED=/dev/ttyS0 cargo test -- --ignored denied_port`.

## Organisation

```
src/                      Frontend React + TypeScript
  features/terminal/      xterm.js, contrôle de flux de la sortie, file d'entrée,
                          thèmes, barre de recherche
  features/settings/      réglages de l'application
  features/shortcuts.ts   raccourcis clavier (définition unique)
  features/sessions/      onglets (reducer, reconnexion auto), panneau d'une session
  features/commands/      barre de commandes, diffusion multi-sessions
  features/connections/   connexion rapide, arborescence et éditeur de profils
  features/transfers/     panneau SFTP (navigation, transferts)
  features/serial/        choix du port et réglages de ligne
  hooks/                  glisser-déposer (événements pointeur)
  components/             barre d'onglets et de titre, barre d'état, dialogues
  services/               appels Tauri typés
  types/                  types partagés avec le backend
src-tauri/src/
  commands/               commandes Tauri (validation, couche fine)
  sessions/               gestionnaire de sessions, boucle par session,
                          automatismes (pager, invite, séquences),
                          journal de session (logger.rs)
  transport/              trait commun, ssh/ (russh, algorithmes modernes/legacy),
                          telnet/ (négociation RFC 854/1073/1091), net.rs (TCP),
                          serial/ (tokio-serial, diagnostic du groupe dialout)
  transfers/              SFTP : listing, envoi/réception, progression, annulation
  forwarding/             redirections de port -L / -D (écoute locale, SOCKS5, relais)
  persistence/            SQLite : migrations, profils, dossiers, réglages
  security/               trousseau (secrets), known_hosts
  errors/                 type d'erreur central { code, message }
  platform.rs             différences Linux / Windows (droits des fichiers, ports série)
src-tauri/migrations/     migrations SQL versionnées
src-tauri/linux/          modèle du fichier .desktop (menu des applications)
src-tauri/icons/          icônes générées (npx tauri icon assets/netforge-icon.svg)
assets/netforge-icon.svg  icône source
scripts/                  serveur SSH de test
packaging/arch/           PKGBUILD et fichier .desktop pour Arch Linux
.github/workflows/        CI (Ubuntu + Windows) et construction des installateurs
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
- L'accès au presse-papiers est activé pour la page (nécessaire au collage sous
  WebKitGTK) : acceptable car elle n'exécute que le code de NetForge (CSP stricte,
  aucun contenu distant).
