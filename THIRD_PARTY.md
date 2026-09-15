# Third-party components — inspection du 15 septembre 2026

Le dossier de travail local contient les binaires PhaseLimiter v0.2.0 et FFmpeg 8.1, mais ils sont exclus du dépôt Git. L'intégration appelle un exécutable séparé ; ce document ne constitue pas une validation de redistribution d'une distribution publique.

## Code inspecté

- **PhaseLimiter** : [licence MIT](https://github.com/ai-mastering/phaselimiter/blob/master/LICENSE). Conserver le copyright et le texte complet avec une distribution. Son DSP n'a pas été modifié.
- **phaselimiter-gui** : [MIT, Shin Fukuse, 2023](https://github.com/ai-mastering/phaselimiter-gui/blob/master/LICENSE). Utilisée comme référence de protocole ; le programme Go/GTK et ses DLL ne sont pas distribués par LYTE.
- **FFmpeg** : [page officielle des licences](https://ffmpeg.org/legal.html). LGPL 2.1 ou ultérieure par défaut ; certaines options rendent le binaire GPL. Une compilation avec des composants `nonfree` peut être non redistribuable. Examiner le binaire exact avec `ffmpeg -version` et `ffmpeg -L`, conserver licence, configuration et accès aux sources correspondantes. L'exécution séparée n'annule pas les obligations attachées au binaire redistribué.

## Dépendances PhaseLimiter

Les [notices upstream](https://github.com/ai-mastering/phaselimiter/tree/master/.circleci/licenses) et le [workflow Windows](https://github.com/ai-mastering/phaselimiter/blob/master/.github/workflows/build-win.yml) ont été consultés. Les fichiers de licence vérifiés indiquent notamment :

| Composant | Notice fournie par upstream |
| --- | --- |
| Boost | Boost Software License 1.0 |
| libsndfile | LGPL 2.1 |
| Intel IPP et TBB dans cette ancienne distribution | Intel Simplified Software License, avril 2018 |
| CImg | CeCILL-C |
| Eigen | MPL 2.0 |
| Armadillo et optim | Apache 2.0 |
| Zstandard | BSD |

L'archive comporte aussi des notices pour gflags, libsimdpp, hnsw, picojson, libpng, bzip2, zlib et les dépendances de tests. Conserver l'ensemble du répertoire `licenses` de la **release exacte**, pas seulement cette liste. La licence MIT de PhaseLimiter ne remplace pas les licences de ses bibliothèques. En particulier, ne pas supposer que le TBB ancien fourni ici porte la même licence qu'une version moderne.

Le binaire retenu, ses versions, ses liaisons statiques/dynamiques et les licences du cache/dataset doivent encore être vérifiés avant distribution publique. Les conditions LGPL, MPL et CeCILL-C peuvent imposer la fourniture de sources et d'autres éléments ; les conditions Intel doivent être préservées. Le script de préparation copie les notices disponibles, sans certifier leur exhaustivité.

## Application

Vue et Vite : MIT ; TypeScript : Apache 2.0 ; Tauri : MIT ou Apache 2.0. Les paquets npm installés et le graphe Cargo verrouillé constituent l'inventaire concret à utiliser avant publication. WebView2 et le runtime Visual C++ ont leurs propres conditions Microsoft.

Pour le développement local, placer manuellement les binaires et leurs ressources suffit. L'installateur inclut tous les fichiers placés dans les deux dossiers de ressources ; conserver les notices et éléments de conformité avec eux avant de le partager.

### FFmpeg détecté sur cette machine

`ffmpeg -version` a été exécuté : **8.1-essentials_build-www.gyan.dev**, avec `--enable-gpl --enable-version3 --enable-static` et notamment libx264/libx265. Cette compilation relève donc de la variante GPL, pas d'une distribution LGPL minimale. L'exécutable a été copié dans `bin/ffmpeg.exe` pour ce build local autonome, avec LICENSE, README et configuration dans `bin/licenses/ffmpeg/`. Sa présence ne vaut pas validation des obligations pour un futur installateur contenant FFmpeg.


### Runtime local PhaseLimiter

Archive officielle v0.2.0 : https://github.com/ai-mastering/phaselimiter/releases/download/v0.2.0/phaselimiter-win.zip. Le runtime local copie moteur, 11 DLL et cache sans modification. Les notices originales restent dans `resources/phaselimiter/licenses` et `resources/phaselimiter/LICENSE`, qui sont exclus du dépôt avec le runtime. Les empreintes sont consignées dans `runtime-manifest.json` local ; le fournisseur ne publiait pas de digest GitHub pour cette ancienne archive. Le build local est fonctionnel ; avant diffusion publique, réunir les sources correspondantes et satisfaire les obligations des composants redistribués.
