// libretro's databases and thumbnails, both from raw.githubusercontent.com, which answers
// cross-origin requests. thumbnails.libretro.com serves the same images and sends no
// access-control-allow-origin header at all, so a page can fetch it and still not read a byte.

const DAT = 'https://raw.githubusercontent.com/libretro/libretro-database/master/metadat/no-intro/';
const THUMBS = 'https://raw.githubusercontent.com/libretro-thumbnails/';

// One database and one thumbnail repository per platform, under libretro's own name for the
// system: spaces in the database file, underscores in the repository. The platform is the card
// folder a rom sits in, so a .gbc filed under GB is looked up in the Game Boy database — slot
// decides a platform by where a file is, and the studio does not second-guess it by extension.
const NAMES = {
  GBA: 'Nintendo - Game Boy Advance',
  GB: 'Nintendo - Game Boy',
  GBC: 'Nintendo - Game Boy Color',
};

export async function fetchDat(platform) {
  const r = await fetch(`${DAT}${encodeURIComponent(NAMES[platform])}.dat`);
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.text();
}

// A card can hold hundreds of carts, each wanting two images, and GitHub is not a CDN.
export function limiter(slots) {
  let running = 0;
  const waiting = [];
  const pump = () => {
    while (running < slots && waiting.length) {
      const { job, resolve, reject } = waiting.shift();
      running++;
      job()
        .then(resolve, reject)
        .finally(() => {
          running--;
          pump();
        });
    }
  };
  return (job) =>
    new Promise((resolve, reject) => {
      waiting.push({ job, resolve, reject });
      pump();
    });
}

// A set of our own, hosted as static files: <platform>/<media>/<CRC32>.png, with an index.json
// mapping a rom's checksum to what it holds. Looked up by checksum rather than by name, so it
// needs no agreement about spelling and reaches roms No-Intro has never heard of. It is a
// separate origin from the page, so whatever serves it has to allow the read.
export async function fetchIndex(base) {
  const r = await fetch(new URL('index.json', base));
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export async function fetchArt(base, path) {
  const r = await fetch(new URL(path, base));
  if (r.status === 404) return null;
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return new Uint8Array(await r.arrayBuffer());
}

// A thumbnail's bytes, or null when libretro has none. A git symlink comes back as a one-line
// body naming its target in the same folder; `stubTarget`, from the Rust side, tells the two apart.
export async function fetchThumb(platform, folder, name, stubTarget) {
  const repo = NAMES[platform].replaceAll(' ', '_');
  const get = async (file) => {
    const r = await fetch(`${THUMBS}${repo}/master/${folder}/${encodeURIComponent(file)}`);
    if (r.status === 404) return null;
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    return new Uint8Array(await r.arrayBuffer());
  };
  const body = await get(`${name}.png`);
  if (!body) return null;
  const target = stubTarget(body);
  return target === undefined ? body : get(target);
}
