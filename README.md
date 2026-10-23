<div align="center">

<img src="assets/img/logo.png" alt="Vocalibre" width="112">

# Vocalibre

**Votre voix devient du texte, sans rien envoyer sur Internet.**

`Ctrl+Espace`, vous parlez, `Ctrl+Espace` : le texte s'écrit là où se trouve votre curseur.<br>
Dans n'importe quelle application, 100 % en local, accéléré par votre carte graphique.

[![Release](https://img.shields.io/github/v/release/williamloree/Vocalibre?style=flat-square&color=2156d9)](https://github.com/williamloree/Vocalibre/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/williamloree/Vocalibre/ci.yml?branch=develop&style=flat-square&label=CI)](https://github.com/williamloree/Vocalibre/actions/workflows/ci.yml)
[![Licence MIT](https://img.shields.io/badge/licence-MIT-4f8cff?style=flat-square)](LICENSE)
<br>
[![Windows](https://img.shields.io/badge/Windows-0078D4?style=flat-square&logo=windows&logoColor=white)](#installation)
[![macOS](https://img.shields.io/badge/macOS-000000?style=flat-square&logo=apple&logoColor=white)](#installation)
[![Linux](https://img.shields.io/badge/Linux-FCC624?style=flat-square&logo=linux&logoColor=black)](#installation)
[![Rust](https://img.shields.io/badge/Rust-000000?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Tauri 2](https://img.shields.io/badge/Tauri%202-24C8D8?style=flat-square&logo=tauri&logoColor=white)](https://tauri.app)
[![whisper.cpp](https://img.shields.io/badge/STT-whisper.cpp-7c3aed?style=flat-square)](https://github.com/ggml-org/whisper.cpp)

[Installation](#installation) ·
[Utilisation](#utilisation) ·
[Modèles](#modèles) ·
[Comment ça marche](#comment-ça-marche) ·
[Développement](#développement) ·
[Contribuer](#contribuer)

</div>

## Pourquoi Vocalibre

Les dictées vocales intégrées aux systèmes envoient votre voix sur des serveurs, demandent un compte, ou
ne marchent que dans certaines applications. Vocalibre transcrit sur votre ordinateur avec
[Whisper](https://github.com/openai/whisper) : rien ne sort de la machine, ça marche hors ligne, et le texte
arrive dans l'application que vous utilisiez déjà (éditeur, navigateur, messagerie, terminal).

## Fonctionnalités

- 🎙️ **Dictée partout** : un raccourci global, le texte est collé à l'emplacement du curseur.
- 🔒 **100 % local** : aucun audio ni texte ne quitte l'ordinateur. Aucun compte, aucune télémétrie.
- ⚡ **Accélération GPU** (Vulkan sous Windows et Linux, Metal sous macOS), repli automatique sur le processeur.
- 🤫 **Pas de texte inventé** : une détection de voix (Silero VAD) filtre le silence et le bruit avant Whisper,
  et les phrases parasites connues (« Sous-titres réalisés par… ») sont retirées.
- 📋 **Texte gardé dans le presse-papiers** après le collage, pour le recoller ailleurs. Option inverse : restaurer
  l'ancien contenu (texte ou image).
- 🔔 **Retours sonores et pilule flottante** : début, fin, transcription terminée ; forme d'onde en direct.
- 🌍 **Multilingue** : détection automatique de la langue, ou langue forcée. Interface en français et en anglais.
- 📝 **Vocabulaire et remplacements** : noms propres et jargon mieux reconnus ; « virgule » → `,`, « point à la
  ligne » → retour à la ligne.
- 🕘 **Historique** des 50 dernières dictées, copie en un clic, jamais envoyé nulle part (désactivable).
- ⚙️ **Paramètres appliqués immédiatement** : raccourcis, micro, modèle,
  accélération, mode de collage… sans redémarrer.
- ⏹️ `Échap` annule une dictée en cours.

## Installation

Téléchargez l'installeur de votre système dans les [Releases](https://github.com/williamloree/Vocalibre/releases/latest).

| Système | Fichier | Remarques |
|---|---|---|
| **Windows 10/11** | `Vocalibre_x.y.z_x64-setup.exe` | Sans droits administrateur. Si SmartScreen s'affiche : *Informations complémentaires* › *Exécuter quand même*. |
| **macOS 11+** Apple Silicon | `Vocalibre_x.y.z_aarch64.dmg` | Application non notarisée : clic droit › *Ouvrir* la première fois. Autoriser le *Micro* et l'*Accessibilité* (pour coller le texte). |
| **macOS 11+** Intel | `Vocalibre_x.y.z_x64.dmg` | Idem. |
| **Linux** | `.AppImage`, `.deb`, `.rpm` | Raccourci global et collage sous X11. Mises à jour automatiques avec l'AppImage. |

Au premier lancement, ouvrez *Paramètres* › *Modèles* et téléchargez un modèle (voir [Modèles](#modèles)) :
il devient actif dès la fin du téléchargement.

**Mises à jour.** Au démarrage puis une fois par jour, Vocalibre regarde si une nouvelle version est publiée ici
et propose de l'installer ; elle redémarre ensuite toute seule. Les paquets sont signés et vérifiés avant
installation. À la demande : menu de l'icône › *Rechercher des mises à jour*. Désactivable dans
*Paramètres* › *Général*.

## Utilisation

| Action | Raccourci |
|---|---|
| Démarrer / arrêter la dictée | `Ctrl+Espace` (ou *Démarrer la dictée* dans le menu de l'icône) |
| Dicter tant que la touche est enfoncée | `F8` |
| Annuler la dictée en cours | `Échap` |
| Recoller la dernière dictée | Raccourci à définir dans *Paramètres* › *Raccourcis*, ou menu de l'icône › *Copier la dernière dictée* |
| Changer de modèle | Clic sur l'icône › *Modèle* |
| Paramètres, historique | Clic sur l'icône › *Paramètres* ou *Historique* |
| Déplacer la pilule | Glisser-déposer pendant une dictée (position mémorisée par écran) |

Un enregistrement de moins de 0,3 s est ignoré (appui accidentel). Une dictée est limitée à 5 minutes
(réglable). Si une dictée suit la précédente de moins d'une minute, une espace est ajoutée devant.

## Modèles

Les modèles se téléchargent depuis *Paramètres* › *Modèles*, avec le petit modèle de détection de voix
qui les accompagne. Un téléchargement coupé reprend où il s'est arrêté ; chaque fichier est vérifié
(SHA-256) avant d'être utilisé. Hors ligne, déposez les fichiers vous-même dans le dossier des modèles
(*Ouvrir le dossier*) : le modèle de détection de voix
[`ggml-silero-v6.2.0.bin`](https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin)
et au moins un modèle Whisper :

| Modèle | Taille | Pour | Phrase de 4 s transcrite en |
|---|---|---|---|
| [Large v3 Turbo Q5](https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin) | 574 Mo | Carte graphique (recommandé) | 0,29 s (RTX 2060 Super) |
| [Large v3 Turbo Q8](https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q8_0.bin) | 874 Mo | Carte graphique | 0,39 s |
| [Large v3 français (distil)](https://huggingface.co/bofenghuang/whisper-large-v3-french-distil-dec16) | 791 Mo | Carte graphique, français uniquement | 0,49 s |
| [Small Q5](https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin) | 190 Mo | Processeur récent | 0,53 s (Ryzen 7 3700X) |
| [Base Q5](https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base-q5_1.bin) | 60 Mo | Petit processeur | 0,18 s, moins précis |

Le modèle français se télécharge sous le nom `ggml-model-q5_0.bin` : le renommer en
`ggml-large-v3-french-distil-dec16-q5_0.bin`. Noms de fichiers, tailles et empreintes SHA-256 : [`models/manifest.json`](models/manifest.json).
Mesures complètes : [`docs/benchmark.md`](docs/benchmark.md).

## Comment ça marche

```text
 Ctrl+Espace ─► Micro (cpal) ─► 16 kHz mono ─► VAD Silero ─► Whisper (GPU) ─► Nettoyage ─► Ctrl+V
                    │                              │                                         │
              forme d'onde                pas de voix = rien                 texte dans le presse-papiers
```

- **Une machine à états pure** décide de tout (démarrer, arrêter, annuler, ignorer les appuis trop rapprochés).
  Elle ne fait aucune entrée/sortie et chaque couple état × événement est testé.
- **Un acteur** est le seul à modifier l'état ; le raccourci, l'icône, le micro et Whisper lui envoient des
  messages. Rien ne bloque le raccourci : la capture et la transcription tournent sur leurs propres threads.
- **Audio.** Le callback du micro ne fait que remplir un tampon circulaire sans verrou ; le rééchantillonnage
  vers 16 kHz se fait à l'arrêt.
- **Transcription.** Le VAD isole les passages parlés ; s'il n'y en a pas, Whisper n'est pas appelé (c'est sur
  le silence qu'il invente du texte). Recherche en faisceau sur GPU, décodage glouton sur processeur.
- **Injection.** Collage du texte, qui reste dans le presse-papiers ; si l'option est désactivée, l'ancien
  contenu est restauré après 300 ms. Mode *frappe*
  disponible pour les applications qui bloquent le collage.

## Développement

Prérequis : [Rust](https://rustup.rs) 1.85+, [Node.js](https://nodejs.org) 22, [CMake](https://cmake.org) et
[LLVM](https://llvm.org) (pour compiler whisper.cpp), le [SDK Vulkan](https://vulkan.lunarg.com) sous Windows
et Linux, et [Ninja](https://ninja-build.org) sous Windows. Sous Linux, ajouter les
[dépendances de Tauri](https://tauri.app/start/prerequisites/) et `libasound2-dev`.

Variables d'environnement attendues : `VULKAN_SDK` (posée par l'installeur du SDK), `LIBCLANG_PATH` vers le
dossier `bin` de LLVM, et CMake dans le `PATH`. Après les avoir installés, ouvrez un **nouveau** terminal
(sous VS Code, redémarrez l'éditeur entier) : un terminal déjà ouvert ne les voit pas.

```sh
git clone https://github.com/williamloree/Vocalibre.git
cd Vocalibre
npm install
npm run tauri dev
```

Le premier build compile whisper.cpp et ses shaders Vulkan : comptez quelques minutes.

| Dossier | Rôle |
|---|---|
| [`src-tauri/src/app`](src-tauri/src/app) | Machine à états et acteur central |
| [`src-tauri/src/audio`](src-tauri/src/audio) | Capture micro, rééchantillonnage, niveaux, silence |
| [`src-tauri/src/stt`](src-tauri/src/stt) | Whisper + VAD sur un thread dédié, nettoyage du texte |
| [`src-tauri/src/injection`](src-tauri/src/injection) | Collage via le presse-papiers ou frappe |
| [`src-tauri/src/settings`](src-tauri/src/settings) | Paramètres : validation, migrations, écriture atomique |
| [`src-tauri/src/models`](src-tauri/src/models) | Catalogue des modèles |
| [`src-tauri/src/shell.rs`](src-tauri/src/shell.rs) | Icône, pilule, fenêtre, raccourcis globaux |
| [`src-tauri/src/commands.rs`](src-tauri/src/commands.rs) | Commandes appelées par l'interface (autorisées fenêtre par fenêtre) |
| [`src`](src) | Interface : pilule et paramètres (React, TypeScript, Vite, i18next) |
| [`tools/bench`](tools/bench) | Banc d'essai des modèles |

**Tests** : `cargo test --workspace`. Le test d'intégration
[`src-tauri/tests/stt_fixtures.rs`](src-tauri/tests/stt_fixtures.rs) transcrit les fichiers de
[`tests/fixtures`](tests/fixtures) (silence, bruit, français, anglais) et vérifie le taux d'erreur ; il
s'exécute quand `VOCALIBRE_TEST_MODELS` pointe vers un dossier contenant `ggml-base-q5_1.bin` et le modèle VAD.

**Qualité** : `cargo fmt --all --check` et `cargo clippy --workspace --all-targets -- -D warnings` doivent
passer sans avertissement.

**Publier une version** : mettre le même numéro dans `Cargo.toml` (`[workspace.package]`),
`src-tauri/tauri.conf.json` et `package.json`, committer, puis pousser le tag `vX.Y.Z`. Le workflow
[`release.yml`](.github/workflows/release.yml) refuse un tag qui ne correspond pas, construit et signe les
installeurs des trois systèmes, et publie la release avec le `latest.json` lu par les applications installées.
Il a besoin des secrets `TAURI_SIGNING_PRIVATE_KEY` et `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Chaque push sur
`develop` passe par [`ci.yml`](.github/workflows/ci.yml) : lint, tests et installeurs de test téléchargeables
depuis la page du run.

**Journaux** : un fichier par jour, gardé 7 jours (*Paramètres* › *Avancé*). Le
texte dicté n'y figure jamais, sauf si `debug_log_text` est activé. Niveau réglable avec `VOCALIBRE_LOG=debug`.

## Feuille de route

- [x] Banc d'essai des modèles et choix des modèles par défaut
- [x] Dictée de bout en bout : raccourci, micro, VAD, Whisper GPU, collage, sons, pilule flottante
- [x] Fenêtre de paramètres (tous les réglages appliqués à chaud) et historique des dictées
- [x] Interface en français et en anglais, mode « maintenir pour parler »
- [ ] Assistant de premier lancement : détection du matériel et téléchargement des modèles
- [x] Installateurs Windows, macOS, Linux construits par GitHub Actions, mises à jour signées dans l'application
- [ ] Signature des installeurs auprès de Windows et d'Apple (SmartScreen, Gatekeeper)

## Limites connues

- Testé uniquement sous Windows pour l'instant ; macOS et Linux compilent mais ne sont pas encore éprouvés.
- Le collage est refusé par les applications lancées en administrateur (protection de Windows). Ne lancez pas
  Vocalibre en administrateur pour contourner : utilisez l'application cible en mode normal.
- Sous Linux, le raccourci global et le collage demandent X11 (Wayland ne les autorise pas).
- `Ctrl+Espace` est aussi l'autocomplétion de plusieurs éditeurs de code : changez-le dans
  *Paramètres* › *Raccourcis* si besoin.
- Sur carte graphique, la toute première dictée après une installation ou une mise à jour attend que le pilote
  compile ses shaders (jusqu'à 30 s, pendant « Chargement du modèle… ») ; c'est ensuite mis en cache.
- Les versions publiées demandent un processeur x86-64 avec AVX2 (depuis 2013 chez Intel, 2015 chez AMD) ou un
  Mac Apple Silicon.
- La carte graphique est partagée : un jeu ou un rendu en cours ralentit la transcription.

## Contribuer

Les contributions sont les bienvenues : signalez un bug ou proposez une idée dans les
[issues](https://github.com/williamloree/Vocalibre/issues), ou ouvrez une pull request vers `develop`.
Avant d'envoyer du code, vérifiez que les tests, `cargo fmt` et `cargo clippy` passent. Messages de commit au
format [Conventional Commits](https://www.conventionalcommits.org/fr/).

## Licence

Distribué sous [licence MIT](LICENSE). Whisper (OpenAI) et Silero VAD sont sous licence MIT ; les modèles
téléchargés gardent leur propre licence.

<div align="center">
<sub>Fait avec Rust, Tauri et whisper.cpp · Si Vocalibre vous est utile, une ⭐ fait plaisir.</sub>
</div>
