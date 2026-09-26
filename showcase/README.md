# Vitrine LYTE

Cette vitrine est un site statique autonome, prévu pour `mastering.mathieuluyten.be`.

## Publication

1. Configurer le sous-domaine pour servir le contenu du dossier `showcase/`.
2. Déployer ce dossier tel quel (sans étape Node.js).
3. Vérifier que `https://mastering.mathieuluyten.be` est servi en HTTPS.

Le script `app.js` demande la dernière release publique de `MLyte/LYTE-Mastering` à l’API GitHub. S’il trouve un asset `.exe`, les boutons téléchargent cet installateur directement ; sinon ils ouvrent la dernière release. La version détectée prend le relais du lien direct vers la bêta 0.3.0-beta.2, conservé dans les pages et utilisé si la vérification GitHub échoue.

À noter : les polices sont chargées depuis Google Fonts. Pour une vitrine entièrement sans requête tierce, les auto-héberger avant la mise en production.

## Référencement et aperçus de partage

Les pages FR (`/`) et EN (`/en.html`) contiennent les métadonnées Open Graph et Twitter, une URL canonique, les liens `hreflang` réciproques et les données structurées WebSite, WebPage et SoftwareApplication. La description et le titre principal précisent le mastering audio gratuit pour Windows. Aucun avis ni score utilisateur n’est inventé dans les données structurées.

Déployer **tout le dossier showcase**, y compris `assets/`, `robots.txt` et `sitemap.xml`. Les adresses absolues ciblent `https://mastering.mathieuluyten.be` : les modifier si le domaine change. La variante `/index.html` pointe vers `/` comme URL canonique ; une redirection permanente côté hébergement peut compléter cette indication.

Les images de partage françaises et anglaises sont des PNG de 1200 × 630 pixels. Pour les régénérer depuis le dépôt :

```powershell
node scripts/generate-share-images.mjs
```

Le générateur utilise Playwright et Microsoft Edge installé ; `PLAYWRIGHT_CHANNEL` permet de choisir un autre canal installé. Aucune génération ni dépendance Node.js n’est nécessaire sur l’hébergement.

Après publication :

- Vérifier que les deux pages, les images, `/robots.txt` et `/sitemap.xml` répondent en HTTPS avec HTTP 200 et le bon type MIME, sans authentification ni blocage des robots.
- Ajouter le site à Google Search Console puis soumettre `https://mastering.mathieuluyten.be/sitemap.xml` et demander une inspection des deux URL. Cette étape exige l’accès au compte et la validation de propriété.
- Tester un nouveau partage des deux URL. Les plateformes peuvent conserver les anciens aperçus en cache ; l’affichage et les recadrages restent propres à chacune.
- En cas de remplacement ultérieur des images, changer aussi leur nom dans les métadonnées pour renouveler leur URL.

Références : [titres Google](https://developers.google.com/search/docs/appearance/title-link), [versions linguistiques](https://developers.google.com/search/docs/specialty/international/localized-versions), [Open Graph](https://ogp.me/). Les métadonnées facilitent la compréhension du site, sans garantir une position ni un résultat enrichi.
