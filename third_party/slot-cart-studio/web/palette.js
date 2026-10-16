const rgb = (value) => {
  const hex = value.split(' ')[1];
  return [0, 2, 4].map((at) => parseInt(hex.slice(at, at + 2), 16));
};

const chroma = ([r, g, b]) => Math.max(r, g, b) - Math.min(r, g, b);
const light = ([r, g, b]) => r * 0.2126 + g * 0.7152 + b * 0.0722;

function hue([r, g, b]) {
  const max = Math.max(r, g, b);
  const d = chroma([r, g, b]);
  const h = max === r ? ((g - b) / d) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  const deg = (h * 60 + 360) % 360;
  return deg >= 350 ? deg - 360 : deg;
}

export function arrange(swatches) {
  const neutral = (s) => chroma(rgb(s.value)) < 24;
  return {
    neutrals: swatches.filter(neutral).sort((a, b) => light(rgb(b.value)) - light(rgb(a.value))),
    colours: swatches.filter((s) => !neutral(s)).sort((a, b) => hue(rgb(a.value)) - hue(rgb(b.value))),
  };
}
