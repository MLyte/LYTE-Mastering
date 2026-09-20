# Vitrine LYTE

Cette vitrine est un site statique autonome, prévu pour `mastering.mathieuluyten.be`.

## Publication

1. Configurer le sous-domaine pour servir le contenu du dossier `showcase/`.
2. Déployer ce dossier tel quel (sans étape Node.js).
3. Vérifier que `https://mastering.mathieuluyten.be` est servi en HTTPS.

Le script `app.js` demande la dernière release publique de `MLyte/LYTE-Mastering` à l’API GitHub. S’il trouve un asset `.exe`, les boutons téléchargent cet installateur directement ; sinon ils ouvrent la dernière release. Cette stratégie évite de figer un numéro de version ou un nom de fichier dans la page.

À noter : les polices sont chargées depuis Google Fonts. Pour une vitrine entièrement sans requête tierce, les auto-héberger avant la mise en production.
