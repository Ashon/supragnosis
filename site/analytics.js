// Visits to supragnosis.dev are counted with Google Analytics, as the Privacy page says
// (site/privacy/): the pages viewed, the site a visitor came from, and which install command they
// copy (the landing's Copy buttons call window.gtag). Only on the live host: a local preview
// (`task site`) counts nothing, and the app, the server and the CLI have none of it.
(function () {
  if (location.hostname !== "supragnosis.dev") return;
  var id = "G-C6FGV12V7E";
  window.dataLayer = window.dataLayer || [];
  // gtag.js reads each call as an arguments object, not an array.
  window.gtag = function () { window.dataLayer.push(arguments); };
  window.gtag("js", new Date());
  window.gtag("config", id);
  var tag = document.createElement("script");
  tag.async = true;
  tag.src = "https://www.googletagmanager.com/gtag/js?id=" + id;
  document.head.appendChild(tag);
})();
