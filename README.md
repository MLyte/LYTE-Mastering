# LYTE Mastering — MVP Windows

Vue 3 + TypeScript + Vite + Tauri 2. Une fenêtre, traitement audio local par **le véritable PhaseLimiter**, aucun DSP réimplémenté. Aucun serveur HTTP, compte, upload ou backend web. L'application ne télécharge pas de moteur audio.

## Lancer sur cette machine

```powershell
cd C:\www\FREEMASTER
. .\scripts\use-local-rust.ps1
npm.cmd install --cache .npm-cache
npm.cmd run dev
```

Le script Rust active uniquement la chaîne locale `.tools` dans le terminal courant. Sur une autre machine, installer Rust stable MSVC via [rustup](https://rustup.rs/), Visual Studio Build Tools avec **Desktop development with C++** et le SDK Windows, puis omettre cette ligne. Tauri nécessite également Microsoft Edge WebView2. [Prérequis Tauri Windows](https://v2.tauri.app/start/prerequisites/#windows).

`npm run dev` compile Vue puis lance Tauri avec `--no-dev-server`. Aucune URL localhost ni serveur Vite. Après une modification Vue/CSS, relancer la commande. La politique CSP mentionne `ipc.localhost`, qui est le transport IPC de Tauri sous Windows, pas un serveur web applicatif.

## Moteur audio local

Le dossier de travail local contient **PhaseLimiter v0.2.0**, ses **11 DLL Windows**, le cache de la même archive et **FFmpeg 8.1**. Ces exécutables, les builds et les artefacts de test sont volontairement exclus du dépôt Git : FFmpeg dépasse la limite GitHub de 100 Mo et leur redistribution demande une vérification de licence séparée.

Après un clone, importer une distribution locale avec `scripts/prepare-runtime.ps1`, puis lancer `npm.cmd run check:bundle`. L'origine et les empreintes SHA-256 du runtime local sont enregistrées dans `resources/phaselimiter/runtime-manifest.json` lorsqu'il est préparé.

Un vrai mastering a été vérifié depuis l'interface Tauri, sans simuler les commandes Rust : WAV 24 bits / 48 kHz, FLAC, MP3 et échantillon audio upstream. Avec les réglages −8,5 dB / 1,00 / Preserve bass, le dernier test audio a reçu **146 événements de progression**, terminé en environ **17 secondes** et produit un WAV stéréo 16 bits / 44,1 kHz décodable. Mesure FFmpeg EBU R128 sur cet échantillon de 6,35 s : **−14,8 LUFS avant, −8,6 LUFS après**. Ce résultat confirme le traitement, sans constituer une évaluation subjective de qualité sur tous les styles.

## Remplacer les binaires (optionnel)

Les étapes ci-dessous servent à renouveler les composants déjà présents.

1. Télécharger volontairement une archive Windows x64 upstream depuis les [releases PhaseLimiter](https://github.com/ai-mastering/phaselimiter/releases) ou les [releases de sa GUI](https://github.com/ai-mastering/phaselimiter-gui/releases). Extraire l'archive. Il faut le sous-dossier **phaselimiter**, contenant `bin` et `resource`, pas uniquement l'ancien exécutable GTK.
2. Depuis ce projet, importer cette arborescence locale :

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\prepare-runtime.ps1 -PhaseLimiterDirectory "C:\Downloads\release\phaselimiter"
```

Adapter uniquement le chemin d'archive. Le script copie `phase_limiter.exe`, ses DLL, le cache binaire et les notices disponibles. Il ne télécharge rien. On peut aussi copier ces fichiers manuellement :

```text
bin/
  phase_limiter.exe
  ffmpeg.exe                 # facultatif en développement si présent dans PATH
  [DLL de la même archive]
resources/phaselimiter/
  sound_quality2_cache       # fichier non vide, sans extension
  LICENSE / licenses/        # notices de la distribution retenue
```

3. Installer si nécessaire le [Microsoft Visual C++ Redistributable x64](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist). Garder ensemble l'exécutable et ses DLL de la même distribution. Le workflow Windows upstream copie : `sndfile.dll`, `boost_system.dll`, `boost_filesystem.dll`, `boost_serialization.dll`, `boost_math_tr1.dll`, `boost_iostreams.dll`, `tbb.dll`, `tbbmalloc.dll`, `libbz2.dll`, `zlib.dll`, `zstd.dll`. Une autre release peut avoir des dépendances différentes : ne pas mélanger les versions.
4. FFmpeg est inclus dans `bin` avec sa licence et ses informations de build. Pour le remplacer, conserver les éventuelles DLL et notices de la nouvelle distribution. Le script accepte `-FfmpegPath "C:\chemin\ffmpeg.exe"` pour copier l'exécutable ; copier les autres fichiers nécessaires de sa distribution séparément.
5. Vérifier les présences :

```powershell
npm.cmd run check:runtime
```

La présence des fichiers ne garantit pas la compatibilité des DLL. Un vrai mastering reste le test final.

## Inspection upstream et intégration

Fichiers inspectés : [main.go](https://github.com/ai-mastering/phaselimiter-gui/blob/master/main.go), [mastering.go](https://github.com/ai-mastering/phaselimiter-gui/blob/master/mastering.go), [go.mod](https://github.com/ai-mastering/phaselimiter-gui/blob/master/go.mod), README, licences, CMake, workflow Windows et `src/phase_limiter/main.cpp` du moteur.

La GUI Go/GTK lance un processus avec `exec.Command`, lit sa sortie et extrait `progression:`. `go.mod` référence gotk3 ; ni Go ni GTK ne sont nécessaires ici. Le moteur attendu est `phase_limiter.exe`. Le cache est un fichier de données sérialisées fourni avec la release, utilisé par `mastering5`.

Le Rust conserve les paramètres upstream :

| Réglage | Arguments |
| --- | --- |
| Target loudness | `--reference` |
| Mastering intensity | `--mastering_matching_level`, `--mastering_ms_matching_level`, `--mastering5_mastering_level` |
| Preserve bass | `--erb_eval_func_weighting true/false` |
| Mode fixe | `--mastering true --mastering_mode mastering5` |
| Fichiers | `--input`, `--output`, `--ffmpeg`, `--sound_quality2_cache` |

Aucun changement des valeurs DSP restantes : les valeurs par défaut inspectées produisent du WAV 16 bits / 44,1 kHz. Le réglage est nommé dB comme upstream, sans promesse de mesure LUFS différente.

La commande s'exécute dans `spawn_blocking`. Deux lecteurs vident simultanément stdout/stderr ; seuls les pourcentages sont émis vers Vue. La progression reste au plus à 99 % jusqu'à la réussite et l'enregistrement du fichier. Les détails techniques restent dans la console de développement.

Le traitement utilise un répertoire temporaire unique et inscriptible, contenant une copie du morceau avec un nom simple et le dossier `tmp` attendu par le moteur Windows. Les chemins du moteur et du cache sont absolus. FFmpeg est résolu et vérifié par chemin absolu ; son dossier est placé en tête du PATH du seul processus enfant et le moteur reçoit `--ffmpeg ffmpeg.exe`. Cette adaptation évite le défaut upstream de citation du chemin FFmpeg dans `std::system` lorsque le dossier d'installation contient des espaces, sans changer le DSP. Cela préserve le source et évite d'écrire dans le répertoire d'installation. Prévoir l'espace disque pour la copie, les intermédiaires et le master.

Le résultat est `<original>_mastered.wav` à côté du source. Les masters existants ne sont jamais écrasés ; les fichiers partiels sont temporaires. `Open folder` utilise uniquement le dernier résultat enregistré côté Rust. Les réglages sont validés côté Rust et mémorisés localement côté Vue. Un seul traitement à la fois ; la fermeture normale de la fenêtre est empêchée pendant le traitement (pas d'annulation dans ce MVP).

## Compiler PhaseLimiter soi-même (optionnel)

La voie simple reste le binaire Windows upstream. Le [workflow Windows](https://github.com/ai-mastering/phaselimiter/blob/master/.github/workflows/build-win.yml) utilise MSVC, CMake, les sous-modules Git, Conda et les dépendances historiques suivantes :

```powershell
git clone --recurse-submodules https://github.com/ai-mastering/phaselimiter.git
cd phaselimiter
conda create -n phaselimiter python=3.11
conda activate phaselimiter
conda install -c intel tbb-devel=2019.2 ipp-include=2019.2 ipp-static=2019.2
conda install -c conda-forge boost=1.82.0 armadillo=12.6.2 libpng=1.6.39
$env:CONDA_ROOT = $env:CONDA_PREFIX
cmake -S . -B build -A x64 -DCMAKE_BUILD_TYPE=Release -DSANDYBRIDGE_SUPPORT=ON -DDISABLE_TARGET_BENCH=ON -DDISABLE_TARGET_TEST=ON
cmake --build build --config Release
```

Cette recette est dérivée du workflow upstream, **non exécutée ici** ; la disponibilité actuelle des anciens paquets Conda n'est pas garantie. Ne pas présenter cela comme un port MSVC déjà validé. Le CMake référence aussi `prebuilt/win64` (libsndfile, optim et headers). Respecter les versions et notices, sans réécrire le DSP. Copier les DLL énumérées plus haut à côté de `build/bin/Release/phase_limiter.exe` et `audio_analyzer.exe`.

Pour fabriquer un cache compatible avec ce build, upstream fait :

```powershell
git clone --depth 1 https://github.com/ai-mastering/bakuage_dataset1.git
.\build\bin\Release\audio_analyzer.exe --mode=sound_quality2_preparation --analysis_data_dir=.\bakuage_dataset1\analysis_shortpath --sound_quality2_cache=.\resource\sound_quality2_cache
```

Ne pas remplacer ce fichier par un cache vide ou le cache texte ProMeter. Python/Conda servent ici à la préparation de l'environnement upstream ; l'appel natif `mastering5` du MVP n'utilise aucun serveur Python. La liste `.so` du README moteur concerne Linux et ne doit pas être copiée telle quelle pour Windows.

## Build de l'application

```powershell
cd C:\www\FREEMASTER
. .\scripts\use-local-rust.ps1
npm.cmd run typecheck
npm.cmd run build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm.cmd run desktop:build
```

Résultats attendus :

- Exécutable : `src-tauri/target/release/lyte-mastering.exe`.
- Installateur Windows NSIS : `src-tauri/target/release/bundle/nsis/*-setup.exe`.

L'installateur inclut `bin/` et `resources/phaselimiter/` tels qu'ils existent au build. Le script `stage-runtime.mjs` copie aussi ces ressources à côté de l'exécutable standalone : la copie de ressources de Tauri pouvait rester périmée après l'ajout de binaires. `npm run desktop:build` vérifie la présence du moteur, du cache, des 11 DLL et du FFmpeg embarqué avant de construire. Refaire le build après remplacement des binaires. Pour déplacer l'exécutable hors de l'installateur, garder les répertoires `bin` et `resources/phaselimiter` à ses côtés. Aucun terminal n'est nécessaire pour utiliser l'application installée.

## Vérification fonctionnelle à effectuer avec le moteur

1. Déposer un seul WAV FL Studio ; vérifier nom et format. Tester aussi sélection de fichier, FLAC, MP3 et refus d'un fichier non audio.
2. Régler −8,5 dB, 1,00, Preserve bass activé ; cliquer MASTER TRACK.
3. Vérifier que l'interface reste réactive et que les valeurs de progression proviennent du moteur.
4. Attendre Master complete ; ouvrir le dossier ; écouter le WAV et vérifier qu'il est réellement traité.
5. Relancer avec le même nom : refus d'écraser. Tester un fichier déplacé, un dossier non inscriptible et un moteur/cache manquant.

La durée audio et le choix d'un autre dossier de sortie sont laissés hors de cette première version.

## Fichiers principaux

- `src/App.vue` : interface, états, import Tauri, écoute de progression.
- `src/style.css` : interface dark compacte, aucun framework UI.
- `src-tauri/src/main.rs` : validation, processus, progression, sauvegarde et ouverture du dossier.
- `src-tauri/tauri.conf.json` et `capabilities/default.json` : fenêtre, ressources, CSP et permissions.
- `scripts/prepare-runtime.ps1`, `check-runtime.ps1` : préparation locale explicite et diagnostic.
- `THIRD_PARTY.md` : inspection des licences et limites de redistribution.

## Tests automatisés

```powershell
npm.cmd run test:ui
```

Les trois tests Playwright utilisent Edge installé et une page interceptée en mémoire, **sans serveur**. Ils contrôlent les valeurs par défaut, le layout compact, la sélection, le drop, les arguments transmis, le verrouillage pendant le traitement, la progression, les erreurs et le bouton de sortie. Les commandes Tauri sont simulées dans ces seuls tests : ils ne prouvent pas un mastering audio réel. Les captures de contrôle sont dans `tests/artifacts/`.

Les deux tests Rust vérifient le parseur de progression et le mapping des paramètres upstream. Les tests réels sont séparés et reproductibles :

```powershell
npm.cmd run test:native
$env:LYTE_TEST_FORMAT = 'flac' # ou mp3
npm.cmd run test:native
```

Ils utilisent le build release existant, un signal audio de test généré localement et le véritable moteur. `LYTE_TEST_APP` peut désigner un exécutable release ; `LYTE_TEST_SOURCE` un fichier de test à convertir. L'import est déclenché par un événement de drop Tauri ; les commandes Rust, processus audio, événements de progression et sauvegardes ne sont pas simulés. Le port de débogage WebView2 9237 est utilisé uniquement pendant ces tests, jamais par le lancement normal. Les rapports, captures et fichiers audio restent dans `tests/artifacts/native/`.

## Vérifications réalisées sur cette machine

- `npm install --cache .npm-cache` : réussi (le cache système était inaccessible au premier essai).
- TypeScript et build Vue : réussis.
- `cargo check` : réussi.
- `cargo test` : 2 tests réussis.
- `npm run test:ui` : 3 tests Edge réussis, avec IPC simulé.
- Build Tauri Windows : exécutable et installateur NSIS générés.
- Démarrage natif release et développement : fenêtre **LYTE Mastering** présente et répondant, puis fermée proprement. Le test natif a nécessité de sortir du bac à sable d'exécution pour permettre le profil WebView2.
- WebView2 et Visual C++ runtime : présents. FFmpeg 8.1 : lancé avec succès.
- **Mastering réel vérifié** : WAV, FLAC, MP3, puis échantillon audio upstream ; progression native, décodage des sorties et refus d'écrasement validés. Restent hors de ces tests : appréciation subjective de la qualité audio et installation sur une autre machine.





### Vérification du package complet

Le binaire release et toutes ses ressources ont été copiés dans un dossier contenant des espaces, puis testés par le même parcours natif. Résultat : WAV décodable, 146 événements réels de progression jusqu'à 100 %, environ 16,7 secondes pour l'échantillon upstream et refus d'écraser le master existant. Ce test a permis de corriger la citation du chemin FFmpeg par le moteur upstream. Rapport : tests/artifacts/native/audio-3vKbMb/report.json.
