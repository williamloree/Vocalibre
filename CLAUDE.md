# CLAUDE.md — Vocalibre

Ce fichier sert aux agents (Claude Code et autres) qui travaillent sur ce dépôt. Il complète le
[README](README.md) (fonctionnement, installation, architecture) avec ce qui ne se devine pas en lisant le
code : les règles du projet, les pièges déjà rencontrés et la façon de publier. Lisez-le en entier avant de
modifier quoi que ce soit.

## Le projet

Vocalibre est une application de dictée vocale **100 % locale**, qui vit dans la zone de notification. Un
raccourci global (`Ctrl+Espace` pour démarrer et arrêter, `F8` à maintenir) enregistre le micro. Le VAD Silero
isole la parole, Whisper la transcrit (sur GPU si possible), puis le texte est collé dans l'application active.

- Pile : Rust (workspace : `src-tauri`, `tools/bench`), Tauri 2, whisper-rs `=0.16.0` /
  whisper-rs-sys `=0.15.0` (Vulkan sous Windows et Linux, Metal sous macOS), React 19 + TypeScript + Vite,
  i18next (fr, en).
- Dépôt public `williamloree/Vocalibre`, licence MIT. Branches : `develop` (travail) et `main` (publiée).
- Le cahier des charges est dans `input/`, **ignoré par git** : ne jamais le committer, ni rien de
  personnel (adresses e-mail, chemins de la machine de l'auteur, clés).

## Règles de code

- **Aucune dette** : pas de `unwrap()` / `expect()` hors des tests, pas de `TODO` laissé derrière soi, pas de
  code mort. `unsafe` uniquement avec un commentaire `// SAFETY:` qui justifie l'appel (voir `hardware.rs`).
- **Versions exactes** partout : `=x.y.z` dans Cargo, versions sans `^` dans `package.json`, actions GitHub
  épinglées (`@v7.0.1`), SDK épinglés. Une mise à jour de dépendance est un changement à part entière.
- **Qualité, à faire passer avant chaque commit** :
  ```sh
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  npx tsc --noEmit -p .
  ```
- **Changements chirurgicaux** : chaque ligne modifiée doit servir la demande. Suivre le style existant :
  commentaires en anglais dans le code, qui expliquent le *pourquoi*, aussi denses que le code autour.
- **Commits** : Conventional Commits en anglais (`feat:`, `fix(macos):`, `build:`, `ci:`, `style(welcome):`,
  `chore(release): X.Y.Z`). Le corps explique le pourquoi.
- **Textes de l'interface** : toujours dans `src/locales/fr.ts` **et** `src/locales/en.ts`, mêmes clés. Les
  pluriels utilisent `_one` / `_other`.

## Architecture : invariants à respecter

- `src-tauri/src/app/state.rs` est une **machine à états pure** : aucune entrée/sortie, aucune horloge, aucun
  thread. Chaque couple état × entrée est testé. Toute nouvelle transition doit avoir son test.
- `src-tauri/src/app/actor.rs` est le **seul** à modifier l'état. Le raccourci, l'icône, le micro et Whisper
  lui envoient des messages (`Msg`, `Input`), et il exécute les `Effect` renvoyés par la machine.
- `Shell` (trait, implémenté dans `shell.rs`) isole tout ce qui touche à Tauri : fenêtres, pilule, icône,
  raccourcis.
- Le callback audio ne fait que remplir un tampon circulaire sans verrou. Le rééchantillonnage se fait à
  l'arrêt.
- Si le VAD ne trouve aucune parole, Whisper n'est **pas** appelé : il invente du texte sur le silence.
- **Ajouter une commande appelée par l'interface** demande quatre endroits, sinon elle est refusée :
  1. la fonction dans `src-tauri/src/commands.rs` ;
  2. `generate_handler!` dans `src-tauri/src/lib.rs` ;
  3. la liste `COMMANDS` de `src-tauri/build.rs` ;
  4. l'autorisation `allow-<nom-en-kebab>` dans `src-tauri/capabilities/main.json` (ou `overlay.json`).

  Puis l'appel typé dans `src/api.ts`.
- **Paramètres** (`settings/mod.rs`) : tout changement de format incrémente `SCHEMA_VERSION` et ajoute une
  étape à `migrate()`, avec son test. Ne jamais casser le `settings.json` d'un utilisateur existant.
- **Matériel** (`hardware.rs`) : les GPU viennent de l'API des périphériques ggml. `gpu_device` est l'index
  parmi les seuls périphériques GPU et IGPU, dans l'ordre d'énumération.

## Construire en local (Windows)

Pré-requis : Rust 1.85+, Node 22, CMake, LLVM, Ninja, SDK Vulkan `1.4.363.0`. Dans Git Bash, avant cargo ou
npm :

```sh
export VULKAN_SDK='C:\VulkanSDK\1.4.363.0' LIBCLANG_PATH='C:\Program Files\LLVM\bin'
export PATH="/c/Program Files/CMake/bin:/c/VulkanSDK/1.4.363.0/Bin:$PATH"
export CARGO_TARGET_DIR=D:/vrc   # dossier court : voir MAX_PATH plus bas
npm run tauri dev
```

- Le premier build compile whisper.cpp et ses shaders Vulkan, environ 6 min.
- **Lancer l'app de dev à côté de la version installée** : la version installée bloque l'instance de dev
  (plugin single-instance) et ses raccourcis globaux. Il faut soit la quitter, soit utiliser un autre
  identifiant, qui donne aussi des données séparées :
  ```sh
  npm run tauri dev -- --config '{"identifier":"io.github.williamloree.vocalibre.dev"}'
  ```
  Les données de dev sont alors dans `%APPDATA%\io.github.williamloree.vocalibre.dev\`.
- Les modèles de test et du banc d'essai sont dans `.cache/models/` (ignoré par git). Le test d'intégration
  `src-tauri/tests/stt_fixtures.rs` tourne si `VOCALIBRE_TEST_MODELS` pointe vers un dossier qui contient
  `ggml-base-q5_1.bin` et le modèle VAD.
- Journaux de l'application : `%LOCALAPPDATA%\io.github.williamloree.vocalibre\logs` sous Windows,
  `~/Library/Logs/io.github.williamloree.vocalibre/` sous macOS (bouton dans *Paramètres* › *Avancé*).
  Niveau réglable avec `VOCALIBRE_LOG=debug`. Le texte dicté n'y figure jamais, sauf si `debug_log_text`
  est activé.

## Pièges connus (déjà payés une fois)

### Build
- **MAX_PATH sous Windows** : MSBuild échoue dans la compilation imbriquée des shaders Vulkan de whisper.cpp.
  Il faut Ninja (`CMAKE_GENERATOR` dans `.cargo/config.toml`) et un `CARGO_TARGET_DIR` court.
- **Runtime C/C++ statique** (pas de Visual C++ Redistributable à installer) : `+crt-static` côté Rust, et
  `CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded` + `CMAKE_POLICY_DEFAULT_CMP0091=NEW` pour whisper.cpp.
  whisper-rs-sys transmet à CMake toute variable d'environnement `CMAKE_*`, `GGML_*` ou `WHISPER_*`.
- **Cargo ne voit pas** un changement de ces variables : faire `cargo clean -p whisper-rs-sys` en local, et
  changer le suffixe de la clé `rust-cache` dans `.github/actions/setup/action.yml` pour la CI.
- Vérifier les DLL importées par l'exe :
  `"/c/Program Files/LLVM/bin/llvm-objdump.exe" -p vocalibre.exe | grep "DLL Name"`. Seuls `vulkan-1.dll` et
  `api-ms-win-crt-*` (fournis par Windows 10+) sont admis.
- **`vulkan-1.dll`** : `scripts/vulkan-runtime.mjs` (hook `pretauri`, lancé avant chaque `npm run tauri ...`)
  télécharge le chargeur Vulkan signé LunarG, vérifie les sha256 du zip et de la DLL, puis la place dans
  `src-tauri/vendor/`. `tauri.windows.conf.json` la livre à côté de l'exe. Sans pilote Vulkan, ggml ne trouve
  aucun périphérique et whisper tourne sur le processeur, sans erreur.
- **`GGML_NATIVE=OFF`** dans les workflows : sinon whisper.cpp est compilé pour le processeur du runner
  (`-march=native`) et plante ailleurs. Sur x86, la CI active explicitement AVX, AVX2, FMA et F16C
  (x86-64-v3) : les versions publiées demandent AVX2.

### macOS
- **Micro muet** : Tauri signe avec le *hardened runtime*, qui refuse le micro sans l'autorisation
  `com.apple.security.device.audio-input` (`src-tauri/Entitlements.plist`). macOS ne demande alors rien et
  l'app reçoit du silence. Le message `NSMicrophoneUsageDescription` est dans `src-tauri/Info.plist`.
- **Mac Intel** : Metal sur une Intel Iris prenait 20 à 40 s par dictée et faisait planter l'app.
  `hardware.rs` ne propose donc aucun GPU sur `macos` + `x86_64`.
- **Collage** : il faut l'autorisation *Accessibilité* (enigo). Sans elle, la dictée est reconnue mais le
  collage échoue. Elle reste alors dans l'historique.
- **Signature ad hoc** (`signingIdentity: "-"`, pas de compte Apple Developer) : Gatekeeper bloque le premier
  lancement (contournement documenté dans le README) et macOS peut redemander Micro et Accessibilité après
  chaque mise à jour.
- **Pas de Mac pour tester** : le diagnostic passe par les journaux de l'utilisateur
  (`tail -40 ~/Library/Logs/io.github.williamloree.vocalibre/*`) et les rapports de
  `~/Library/Logs/DiagnosticReports/`.
- **CI macOS** : `hdiutil: couldn't eject ... Resource busy` à la création du `.dmg` vient de Spotlight, qui
  indexe l'image. L'action `setup` le désactive (`mdutil -a -i off`).

### CI et release
- `ci.yml` tourne sur chaque push sur `develop` (et les PR) : lint, tests, puis installeurs sur Windows, macOS
  Apple Silicon, macOS Intel et Linux. Quand tout est vert, le job `promote` **avance `main`** en
  fast-forward sur le même commit. `main` ne reçoit que des commits vérifiés : ne jamais committer
  directement sur `main`.
- Un push plus récent sur `develop` annule le run en cours (`concurrency`) : un run « cancelled » n'est pas un
  échec.
- **Publier une version** :
  1. mettre le même numéro dans `Cargo.toml` (`[workspace.package]`), `src-tauri/tauri.conf.json`,
     `package.json`, les deux entrées `version` en tête de `package-lock.json` et les paquets `vocalibre` et
     `bench` de `Cargo.lock` ;
  2. committer `chore(release): X.Y.Z` sur `develop`, pousser, attendre que la CI soit verte (elle avance
     `main`) ;
  3. pousser le tag annoté `vX.Y.Z` sur ce commit. `release.yml` vérifie que le tag correspond aux versions
     (`scripts/check-version.mjs`), crée une release brouillon, construit et signe les installeurs des quatre
     plateformes, vérifie que `latest.json` les contient toutes, puis publie.
- **Mises à jour** : les apps installées lisent `releases/latest/download/latest.json`. Les paquets de mise à
  jour sont signés avec la clé minisign dont la clé publique est dans `tauri.conf.json`. La clé privée est
  dans `~/.tauri` sur la machine de l'auteur et dans les secrets `TAURI_SIGNING_PRIVATE_KEY` et
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. **Ne jamais la committer ni l'afficher.**
- Les pushs vers GitHub (branches, tags) et les publications passent par l'accord explicite de l'utilisateur.

## Travailler avec l'utilisateur

- Il écrit en français. Les réponses sont en français, concises.
- Avant une action difficile à défaire ou visible publiquement (push, tag, release, suppression), demander
  si ce n'est pas déjà explicitement demandé.
- Ne pas perturber le Vocalibre installé qui tourne sur sa machine : tester avec l'identifiant `.dev`.
- Annoncer honnêtement ce qui a été vérifié et ce qui ne l'a pas été (surtout macOS et Linux, testés sur
  d'autres machines que la sienne).
