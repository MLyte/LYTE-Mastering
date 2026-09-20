const repository = "MLyte/LYTE-Mastering";
const fallback = `https://github.com/${repository}/releases/latest`;
const status = document.querySelector("[data-download-status]");
const version = document.querySelector("[data-release-version]");
const date = document.querySelector("[data-release-date]");

function setDownload(url) {
  document.querySelectorAll("[data-download]").forEach((link) => {
    link.href = url;
  });
}

function formatDate(value) {
  return new Intl.DateTimeFormat("fr-BE", { day: "numeric", month: "long", year: "numeric" }).format(new Date(value));
}

async function loadRelease() {
  try {
    const response = await fetch(`https://api.github.com/repos/${repository}/releases/latest`, {
      headers: { Accept: "application/vnd.github+json" }
    });
    if (!response.ok) throw new Error(`GitHub a répondu ${response.status}`);
    const release = await response.json();
    const installer = release.assets.find((asset) => /\.exe$/i.test(asset.name));
    const destination = installer?.browser_download_url || release.html_url || fallback;
    setDownload(destination);
    const releaseName = release.name || release.tag_name || "Dernière version";
    version.textContent = /b[êe]ta/i.test(releaseName) ? releaseName : `${releaseName} · bêta`;
    date.textContent = release.published_at ? `Publiée le ${formatDate(release.published_at)}` : "Disponible sur GitHub";
    status.textContent = installer ? `${installer.name} · téléchargement direct` : "Dernière release trouvée · choisir l’installateur sur GitHub";
  } catch (error) {
    setDownload(fallback);
    version.textContent = "Dernière version · bêta";
    date.textContent = "Consulter GitHub pour télécharger";
    status.textContent = "Lien vers les releases GitHub — disponibilité à vérifier.";
  }
}

loadRelease();
