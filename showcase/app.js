const repository = "MLyte/LYTE-Mastering";
const fallback = "https://github.com/MLyte/LYTE-Mastering/releases/download/v0.3.0-beta.3/LYTE.Mastering_0.3.0-beta.3_x64-setup.exe";
const status = document.querySelector("[data-download-status]");
const version = document.querySelector("[data-release-version]");
const date = document.querySelector("[data-release-date]");
const english = document.documentElement.lang === "en";
const copy = english ? {
  latest: "Latest release", beta: "beta", published: "Published on",
  available: "Available on GitHub", direct: "direct download",
  choose: "Latest release found · choose the installer on GitHub",
  consult: "Visit GitHub to download",
  unavailable: "GitHub check unavailable — download the published installer."
} : {
  latest: "Dernière version", beta: "bêta", published: "Publiée le",
  available: "Disponible sur GitHub", direct: "téléchargement direct",
  choose: "Dernière release trouvée · choisir l’installateur sur GitHub",
  consult: "Consulter GitHub pour télécharger",
  unavailable: "Vérification GitHub indisponible — télécharger l’installateur publié."
};

function setDownload(url) {
  document.querySelectorAll("[data-download]").forEach((link) => {
    link.href = url;
  });
}

function formatDate(value) {
  return new Intl.DateTimeFormat(english ? "en-GB" : "fr-BE", { day: "numeric", month: "long", year: "numeric" }).format(new Date(value));
}

async function loadRelease() {
  try {
    const response = await fetch(`https://api.github.com/repos/${repository}/releases?per_page=10`, {
      headers: { Accept: "application/vnd.github+json" }
    });
    if (!response.ok) throw new Error(`GitHub a répondu ${response.status}`);
    const releases = await response.json();
    const release = releases.find((candidate) => !candidate.draft);
    if (!release) throw new Error("Aucune release publiée n’est disponible");
    const installer = release.assets.find((asset) => /\.exe$/i.test(asset.name));
    const destination = installer?.browser_download_url || release.html_url || fallback;
    setDownload(destination);
    const releaseName = release.name || release.tag_name || copy.latest;
    version.textContent = /b[êe]ta/i.test(releaseName) ? releaseName : `${releaseName} · ${copy.beta}`;
    date.textContent = release.published_at ? `${copy.published} ${formatDate(release.published_at)}` : copy.available;
    status.textContent = installer ? `${installer.name} · ${copy.direct}` : copy.choose;
  } catch (error) {
    setDownload(fallback);
    version.textContent = `LYTE Mastering v0.3.0-beta.3 · ${copy.beta}`;
    date.textContent = copy.consult;
    status.textContent = copy.unavailable;
  }
}

loadRelease();
