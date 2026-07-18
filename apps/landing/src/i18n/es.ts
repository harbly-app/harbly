import type { Dict } from "./index";

// Spanish — translated from the zh-cn/en master copies.
const es: Dict = {
  langName: "Español",
  htmlLang: "es",
  title: "Harbly — Base de conocimiento local-first en HTML",
  description:
    "Guarda tu conocimiento en páginas web que siempre se podrán abrir. Harbly es una base de conocimiento local-first construida sobre HTML de archivo único: captura páginas web, escribe documentos de página y Markdown, previsualiza en un sandbox, busca a texto completo y comparte enviando un solo archivo. Código abierto, AGPL-3.0.",
  nav: {
    features: "Características",
    how: "Cómo funciona",
    download: "Descargar",
    soon: "próximamente",
  },
  chips: ["Local-first", "Código abierto · AGPL-3.0", "Primero en macOS"],
  hero: {
    h1Pre: "Tu conocimiento, en el único formato ",
    h1Em: "hecho para durar",
    h1Post: "",
    lede: "Abierta, autocontenida y legible décadas después: la página web es el contenedor más fiable para el conocimiento. Harbly es una base de conocimiento local-first construida sobre HTML de archivo único: captura, escribe, organiza y comparte, con todo guardado como archivos de texto plano en tu propio disco.",
    ctaPrimary: "Descargar para macOS",
    ctaSecondary: "Ver las características",
    trust:
      "Gratis · Tus datos siempre en local · Archivos de texto plano que conservas incluso tras desinstalar",
  },
  mock: {
    aria: "Maqueta de la app Harbly: árbol de carpetas a la izquierda, cuadrícula de recursos a la derecha",
    search: "Buscar en tu biblioteca  ⌘K",
    locations: "Ubicaciones",
    inbox: "Bandeja de entrada",
    folderActive: "Proyectos",
    folderSub: "Nuevos precios",
    folderB: "Apuntes",
    tagsLabel: "Etiquetas",
    tagA: "Importante",
    tagB: "Ideas",
    cards: [
      {
        t: "Notas de lectura · Clásicos UX.hdoc",
        m: "Apuntes · ayer",
      },
      {
        t: "Panel de revisión trimestral.html",
        m: "Proyectos · hace 2 días",
      },
      {
        t: "Informe semanal del equipo.md",
        m: "Proyectos · hace 3 días",
      },
      {
        t: "Borrador A/B de precios.html",
        m: "Nuevos precios · la semana pasada",
      },
      {
        t: "Guía de viaje · Kioto.html",
        m: "Bandeja de entrada · ahora mismo",
      },
      {
        t: "Experimentos de tipografía.html",
        m: "Apuntes · hace 2 semanas",
      },
    ],
  },
  features: {
    h2: "Como el Finder, pero habla HTML",
    sub: "Cada hábito que ya tienes con tus archivos, más las piezas que una base de conocimiento necesita.",
    items: [
      {
        icon: "🗂️",
        title: "Tus datos son una carpeta",
        desc: "Tu biblioteca es una carpeta normal en el disco: organízala en el Finder y la app lo refleja al instante. Archivos de texto plano que puedes gestionar con git o cualquier nube, y que te llevas contigo.",
      },
      {
        icon: "✍️",
        title: "Documentos de página y Markdown",
        desc: "Editores integrados de documentos de página WYSIWYG y de Markdown: tus notas ya son páginas web, con tres temas — al exportar obtienes una página terminada.",
      },
      {
        icon: "📤",
        title: "Compartir es enviar un archivo",
        desc: "Exporta un HTML de archivo único y envíaselo a cualquiera: se abre directamente en el navegador, sin cuentas, sin instalar nada y funciona sin conexión.",
      },
      {
        icon: "🛡️",
        title: "Vista previa en sandbox, sin red por defecto",
        desc: "Abre con tranquilidad las páginas que recopilas: la vista previa inyecta un CSP estricto y no lanza ni una petición de red; las peticiones externas bloqueadas se cuentan una a una, y puedes permitirlas solo esta vez cuando lo necesites.",
      },
      {
        icon: "⌘K",
        kbd: true,
        title: "Búsqueda de texto completo",
        desc: "SQLite FTS5 indexa títulos y contenido, con tokenización preparada para CJK; resultados según escribes, incluso en una biblioteca sin organizar.",
      },
      {
        icon: "🏷️",
        title: "Etiquetas sincronizadas con el Finder",
        desc: "Las etiquetas viven en los xattr del propio archivo, visibles desde el Finder y Spotlight; cámbialas en cualquiera de los dos lados y el otro se actualiza solo.",
      },
      {
        icon: "⌘Z",
        kbd: true,
        title: "Deshacer e historial de versiones",
        desc: "Eliminar, mover, renombrar, importar: todo se puede deshacer. Cada edición deja una versión a la que puedes volver en cualquier momento.",
      },
      {
        icon: "🤖",
        title: "Asistente de IA, opcional",
        desc: "Usa un agent local gratuito o trae tu propia API key; deja que la IA revise, organice o genere páginas — cada cambio queda en el historial de versiones.",
      },
      {
        icon: "🌐",
        title: "Interfaz en seis idiomas",
        desc: "简体中文, 繁體中文, English, 日本語, 한국어, Español — sigue el idioma del sistema al primer arranque y puedes cambiarlo cuando quieras.",
      },
    ],
  },
  how: {
    h2: "De página web a base de conocimiento en tres pasos",
    steps: [
      {
        title: "Captura y escribe",
        desc: "Arrastra cualquier página web o crea un documento de página o una nota Markdown; el hash de contenido evita duplicados y lo nuevo llega primero a la bandeja de entrada.",
      },
      {
        title: "Organiza y encuentra",
        desc: "Carpetas, etiquetas y favoritos para poner orden; y con ⌘K, la búsqueda de texto completo lo encuentra todo aunque no ordenes nada.",
      },
      {
        title: "Compártelo",
        desc: "Exporta un único archivo HTML y envíaselo a quien quieras: se abre en el navegador y tus datos nunca salen de tu disco.",
      },
    ],
  },
  cta: {
    h2: "No encierres tu conocimiento en la app de otro",
    sub: "Gratuito y de código abierto, almacenado en texto plano, tuyo para siempre — la versión para macOS está casi lista.",
    btn: "Descargar para macOS",
  },
  footer: {
    tag: "Base de conocimiento local-first en HTML",
    meta: "Código abierto bajo AGPL-3.0 · © 2026 Harbly",
  },
};

export default es;
