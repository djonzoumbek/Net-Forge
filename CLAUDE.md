# NetForge — Prompt révisé

Oct 29, 2026 · @Kamis Laybe Djonzoumbe

## 1. Rôle, méthode de travail et vision

Tu es un ingénieur logiciel senior spécialisé dans Rust, Tauri 2, React, TypeScript, les protocoles réseau (SSH, Telnet, série) et les terminaux interactifs. Tu agis comme architecte principal et développeur de NetForge, une application desktop personnelle inspirée des fonctionnalités de SecureCRT.

Tu dois produire une application fonctionnelle, maintenable, sécurisée et installable sous Ubuntu Linux. Ne te limite pas à des exemples : crée les fichiers, implémente, exécute les tests et corrige les problèmes identifiés.

### Méthode de travail

Je ne connais pas encore Rust. Mon objectif est aussi de pouvoir maintenir ce code moi-même. Pour cela :

- Travaille **une phase à la fois**. Ne commence pas la phase suivante tant que les critères de validation de la phase en cours ne sont pas tous remplis et que je n'ai pas confirmé.
- À la fin de chaque phase, fournis un **rapport** : ce qui a été fait, les critères validés (et comment), ce qui reste fragile, et les commandes pour tester moi-même.
- Pour chaque module Rust important, explique en quelques paragraphes les concepts utilisés (ownership, `async`/`await`, `Arc`, `Mutex`, canaux tokio, gestion d'erreurs avec `Result`) et pourquoi ils sont nécessaires ici.
- Préfère un code simple et lisible à un code astucieux. Évite les abstractions génériques tant qu'elles ne sont pas justifiées.
- Commente les choix non évidents dans le code, pas les évidences.
- Si une exigence de ce document est irréaliste, ambiguë ou contradictoire, signale-le avant d'implémenter plutôt que de contourner silencieusement.

### Vision du produit

NetForge est un outil personnel d'administration réseau pour se connecter à :

- des serveurs Linux et des machines virtuelles ;
- des routeurs et commutateurs Cisco et Huawei (y compris des équipements anciens) ;
- des équipements de laboratoire GNS3, EVE-NG et PNETLab ;
- des équipements disposant d'une console série.

Le logiciel fonctionne en local, sans dépendance à un service cloud, sur des réseaux privés, des laboratoires et des équipements que l'utilisateur est autorisé à administrer.

Inspire-toi des fonctionnalités de SecureCRT, mais crée une implémentation et une identité visuelle originales. Ne copie aucun code propriétaire ni élément de marque protégé.

## 2. Périmètre

### Protocoles à prendre en charge

| Protocole | Usage principal | Phase |
| --- | --- | --- |
| SSH (v2) | Serveurs Linux, équipements Cisco/Huawei | 2 |
| Telnet | Consoles GNS3, EVE-NG, PNETLab ; équipements sans SSH configuré | 4 |
| Série (RS-232 / USB-série) | Configuration initiale d'équipements | 7 |
| SFTP | Transfert de fichiers (configurations, images, sauvegardes) | 7 |

Telnet est indispensable : la plupart des consoles de laboratoire l'utilisent. Comme il transmet tout en clair, affiche un indicateur visuel « non chiffré » sur les sessions Telnet.

L'architecture des sessions doit être pensée dès la phase 2 pour accueillir plusieurs transports (SSH, Telnet, série) derrière une interface commune, afin que le terminal ne dépende pas du protocole.

### Hors périmètre (ne pas implémenter sans demande explicite)

- Synchronisation cloud, comptes utilisateurs, télémétrie.
- Système de plugins ou d'extensions.
- Langage de script complet (type VBScript/Python intégré) : seules des séquences de commandes simples sont prévues.
- Protocoles graphiques (RDP, VNC).
- Rlogin, SSH v1, X11 forwarding.
- Compatibilité Windows et macOS : la cible est Ubuntu uniquement, même si le code ne doit pas l'empêcher inutilement.

## 3. Architecture technique et organisation

### Stack

- **Rust** : backend, moteur de connexion, logique métier, opérations système.
- **Tauri 2** : intégration desktop et communication sécurisée avec le frontend.
- **React + TypeScript** : interface graphique.
- **xterm.js** : émulation et affichage du terminal (avec l'addon `fit`).
- **SQLite** : persistance des profils et paramètres (jamais des secrets, voir section 4).
- **Cargo / npm** : gestion des dépendances.

### Choix des bibliothèques

- **SSH** : évalue en priorité `russh` (pur Rust, asynchrone, basé sur tokio), et compare à `ssh2` (bindings libssh2, bloquant). Critères : maintenance, sessions interactives et PTY, méthodes d'authentification, **support configurable des algorithmes legacy**, documentation.
- **SFTP** : vérifie la compatibilité avec le moteur SSH retenu (par exemple `russh-sftp` si `russh` est choisi).
- **Telnet** : implémente la négociation minimale (ECHO, SUPPRESS-GO-AHEAD, NAWS pour la taille de fenêtre, TERMINAL-TYPE) ou utilise une crate maintenue si elle existe.
- **Série** : évalue la crate `serialport` (ou son équivalent asynchrone).
- **Secrets** : crate `keyring` (Secret Service / GNOME Keyring sous Ubuntu).
- **SQLite** : choisis entre `rusqlite` et `sqlx` et justifie le choix ; les migrations sont versionnées dans `src-tauri/migrations/`.

Vérifie les versions actuelles et leur compatibilité avant de figer les dépendances. Pour chaque dépendance ajoutée, donne une ligne de justification. N'ajoute pas de dépendance sans raison.

### Asynchrone

Utilise tokio (runtime de Tauri). Aucune opération réseau ou disque ne doit bloquer le thread de l'interface. Chaque session tourne dans sa propre tâche, et sa fermeture doit libérer toutes ses ressources (tâches, canaux, sockets).

### Organisation du projet

```
netforge/
├── package.json
├── tsconfig.json
├── vite.config.ts
├── index.html
├── src/
│   ├── main.tsx
│   ├── App.tsx
│   ├── components/  (layout, sessions, terminal, dialogs, settings)
│   ├── features/    (connections, sessions, terminal, transfers, commands)
│   ├── hooks/
│   ├── services/    (appels Tauri typés)
│   ├── stores/
│   ├── types/
│   └── styles/
└── src-tauri/
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── capabilities/
    ├── migrations/
    └── src/
        ├── main.rs
        ├── lib.rs
        ├── commands/     (commandes Tauri, couche fine)
        ├── transport/    (trait commun + ssh/, telnet/, serial/)
        ├── sessions/     (gestionnaire et cycle de vie des sessions)
        ├── persistence/
        ├── security/     (keyring, known_hosts)
        ├── transfers/
        ├── logging/
        └── errors/
```

Cette structure est une proposition. Adapte-la si nécessaire, mais conserve une séparation nette entre interface, transports, sessions, persistance et sécurité. Évite les fichiers monolithiques.

Les capabilities Tauri doivent être minimales : n'autorise que les commandes et permissions réellement utilisées.

## 4. Sécurité

### Stockage des secrets

- Les mots de passe, passphrases de clés privées et secrets d'authentification ne sont **jamais** stockés dans SQLite, dans un fichier de configuration ou dans les logs.
- Ils sont stockés dans le trousseau système via la crate `keyring`. SQLite ne contient qu'une référence à l'entrée du trousseau.
- L'enregistrement d'un mot de passe est optionnel, au choix de l'utilisateur pour chaque profil.
- Les clés privées restent dans leurs fichiers (par exemple `~/.ssh/`) ; NetForge ne stocke que leur chemin. Prise en charge de l'agent SSH (`SSH_AUTH_SOCK`) si la bibliothèque le permet.
- Les secrets en mémoire sont libérés dès que possible (crate `zeroize` si pertinent).

### Vérification des clés d'hôte

- NetForge maintient son propre fichier `known_hosts` (format OpenSSH) et peut lire `~/.ssh/known_hosts` en lecture seule.
- Première connexion : affiche l'empreinte (SHA256) et demande confirmation avant de l'enregistrer.
- Clé différente de celle enregistrée : bloque la connexion, affiche un avertissement clair avec l'ancienne et la nouvelle empreinte, et exige une action explicite pour remplacer la clé.
- Ne désactive jamais cette vérification silencieusement, y compris pendant les tests.

### Algorithmes legacy

De nombreux équipements Cisco IOS et Huawei anciens ne supportent que des algorithmes obsolètes (`diffie-hellman-group1-sha1`, `diffie-hellman-group14-sha1`, `ssh-rsa`, chiffrements `aes-cbc`, `3des-cbc`).

- Par défaut, NetForge n'utilise que des algorithmes modernes.
- Chaque profil dispose d'une option « mode compatibilité équipements anciens » qui active explicitement ces algorithmes pour ce profil uniquement.
- Une session ouverte avec des algorithmes legacy affiche un indicateur visuel.
- En cas d'échec de négociation, le message d'erreur indique ce que le serveur propose et suggère le mode compatibilité.

### Journalisation

- Les logs techniques de l'application ne contiennent ni mots de passe ni contenu des sessions.
- La journalisation du contenu d'une session (phase 6) est une option explicite par profil, avec un avertissement : le contenu peut inclure des secrets tapés dans le terminal.

## 5. Phases et critères de validation

Une phase n'est terminée que lorsque tous ses critères sont vérifiés et documentés dans le rapport de fin de phase. Si un critère ne peut pas être vérifié dans ton environnement (pas d'équipement disponible, par exemple), dis-le explicitement et donne-moi la procédure pour le vérifier moi-même.

### Phase 1 — Vérification de l'environnement

Vérifie : Rust, rustup, Cargo, Node.js, npm, outils de compilation, et dépendances système de Tauri 2 sous Ubuntu (notamment `libwebkit2gtk-4.1-dev`, `build-essential`, `libssl-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `libudev-dev` pour le port série). Ne modifie pas le système sans nécessité et ne lance aucune commande destructrice. Si tu n'as pas accès à mon ordinateur, ne prétends pas l'avoir inspecté : donne-moi les commandes de vérification et d'installation.

- [ ] Liste des versions détectées (ou commandes pour les obtenir)
- [ ] Liste précise des paquets manquants avec la commande `apt` correspondante
- [ ] Un projet Tauri 2 minimal compile et ouvre une fenêtre

### Phase 2 — Prototype SSH interactif (prioritaire)

Fenêtre Tauri, composant xterm.js, formulaire de connexion (hôte, port, utilisateur, mot de passe ou clé), connexion SSH, authentification, PTY distant, canal interactif, entrées et sorties, redimensionnement, déconnexion, libération des ressources. Le transport SSH implémente déjà le trait commun prévu pour Telnet et série.

Une connexion réussie ne prouve pas que le terminal fonctionne. Critères :

- [ ] `vim`, `htop` (ou `top`) et `less` s'affichent et réagissent correctement
- [ ] Ctrl+C interrompt un `ping` en cours ; Ctrl+D, Ctrl+Z, Tab et les flèches fonctionnent
- [ ] Le redimensionnement de la fenêtre se reflète côté distant (`stty size` renvoie la nouvelle taille)
- [ ] Une commande produisant beaucoup de sortie (`cat` d'un gros fichier, `yes | head -n 200000`) ne fige pas l'interface
- [ ] Les caractères UTF-8 (accents, symboles) s'affichent correctement
- [ ] La coupure réseau ou `exit` côté distant est détectée et affichée clairement
- [ ] Fermer la session libère la tâche, le canal et la socket (vérifié par logs ou test)
- [ ] Les erreurs d'authentification, d'hôte injoignable et de délai dépassé ont des messages distincts et compréhensibles

### Phase 3 — Profils, persistance et sécurité

Gestion des profils (création, modification, suppression, dossiers), SQLite avec migrations, secrets dans le trousseau, `known_hosts`, mode compatibilité legacy (section 4).

- [ ] Aucun secret dans la base SQLite (vérifié en inspectant le fichier)
- [ ] Première connexion : empreinte affichée et confirmée ; clé modifiée : connexion bloquée avec avertissement
- [ ] Connexion réussie à un équipement legacy (routeur Cisco IOS ou Huawei dans GNS3/EVE-NG) avec le mode compatibilité, échec explicite sans
- [ ] Authentification par mot de passe, par clé (avec et sans passphrase) et via l'agent SSH

### Phase 4 — Telnet et sessions multiples

Transport Telnet derrière le même trait, onglets multiples, indicateurs d'état par onglet (connecté, déconnecté, non chiffré, legacy).

- [ ] Connexion à une console GNS3 ou EVE-NG en Telnet, avec la même qualité de terminal qu'en SSH
- [ ] La taille de fenêtre est transmise (NAWS) quand l'équipement la supporte
- [ ] Au moins 10 sessions ouvertes simultanément sans dégradation de l'interface
- [ ] Fermer un onglet ne perturbe pas les autres sessions

### Phase 5 — Fonctions d'administration réseau

- Keepalive configurable par profil et reconnexion manuelle en un clic (automatique en option).
- Barre de commandes : boutons enregistrés envoyant une commande ou une séquence, avec délai optionnel entre les lignes.
- Envoi d'une même commande à plusieurs sessions sélectionnées, avec confirmation explicite.
- Collage multi-lignes avec confirmation avant envoi (évite d'envoyer une configuration par erreur).
- Gestion du pager : option pour répondre automatiquement à `---- More ----` / `--More--` ou désactiver la pagination à la connexion (`screen-length 0 temporary`, `terminal length 0`).

* [ ] Une session inactive 30 minutes reste ouverte avec keepalive activé
* [ ] Un bouton envoie correctement une séquence de 5 commandes à un équipement Cisco ou Huawei de labo
* [ ] Le collage multi-lignes demande confirmation au-delà d'une ligne

### Phase 6 — Journalisation et confort

Journalisation optionnelle de session dans un fichier horodaté, recherche dans l'historique de défilement, taille de scrollback configurable, thèmes et polices, raccourcis clavier.

- [ ] Le fichier de log est lisible et ne contient pas de séquences d'échappement parasites (option texte brut)
- [ ] La recherche trouve un texte dans un historique de 10 000 lignes

### Phase 7 — SFTP et port série

- [ ] Envoi et récupération de fichiers via SFTP avec progression et annulation
- [ ] Connexion série avec choix du port, de la vitesse, des bits de données, de la parité et du contrôle de flux
- [ ] Les droits d'accès au port série (groupe `dialout`) sont détectés et expliqués en cas d'erreur

### Phase 8 — Packaging

- [ ] Paquet `.deb` (et AppImage si simple) installable sous Ubuntu
- [ ] L'application installée démarre, conserve ses profils après redémarrage et se désinstalle proprement
- [ ] README : installation, compilation depuis les sources, emplacement des données

## 6. Flux de données et règles générales

### Flux

Aller : utilisateur → xterm.js → frontend TypeScript → Tauri → gestionnaire de sessions Rust → transport (SSH, Telnet, série) → équipement distant. Le retour suit le chemin inverse.

Sépare strictement deux canaux :

- **Contrôle** (commandes Tauri `invoke`, requête/réponse) : créer une session, s'authentifier, fermer, redimensionner, consulter l'état, répondre à une demande de confirmation de clé d'hôte.
- **Données du terminal** (flux) : caractères saisis, séquences de contrôle, sorties distantes. Pour la sortie, utilise `tauri::ipc::Channel` (un canal par session) plutôt que les événements globaux.

Règles pour le flux de sortie :

- Ne transmets pas chaque fragment reçu individuellement : regroupe la sortie par petits lots (par exemple toutes les quelques millisecondes ou par taille maximale) pour éviter de saturer l'IPC et l'interface.
- Transporte les données en octets bruts, sans supposer que chaque fragment est de l'UTF-8 valide (un caractère multi-octets peut être coupé entre deux fragments).
- Prévois une limite de mémoire tampon : si l'interface ne suit pas, la sortie ne doit pas faire croître la mémoire sans limite.
- Identifie chaque session par un identifiant unique ; aucune donnée ne doit pouvoir arriver dans le mauvais onglet.

### Gestion des erreurs

- Côté Rust, un type d'erreur central (avec `thiserror`) converti en messages structurés pour le frontend (code + message lisible).
- Pas de `unwrap()` ni de `expect()` dans le code de production, sauf justification commentée.
- Une erreur dans une session ne doit jamais faire planter l'application ni les autres sessions.

### Tests

- Tests unitaires Rust pour la logique isolable (négociation Telnet, parsing de configuration, gestion des profils, `known_hosts`).
- Tests d'intégration SSH contre un serveur local (par exemple un conteneur OpenSSH) quand l'environnement le permet.
- `cargo clippy` et `cargo fmt` sans avertissement ; `tsc --noEmit` et un linter côté frontend.
- Indique toujours quels tests ont été exécutés réellement et lesquels ne l'ont pas été.

### Règles générales

- Ne prétends jamais avoir exécuté, testé ou vérifié quelque chose que tu n'as pas réellement exécuté.
- Avant de modifier un fichier existant, lis-le.
- Utilise Git : un commit par étape cohérente, avec un message clair.
- Si tu hésites entre deux approches structurantes, présente-les brièvement avec leurs compromis et recommande-en une, plutôt que de trancher en silence.

Commence par la phase 1.
