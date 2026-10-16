// The fine print a real GBA cart carries under its label, where the page once had region pills.
// Game Boy carts print nothing: their headers carry no product code to print.
// A GBA game code's last letter is the market it was made for.
const BY_LETTER = { E: 'USA', P: 'EUR', J: 'JPN', D: 'NOE', F: 'FRA', S: 'ESP', I: 'ITA', H: 'HOL', K: 'KOR', U: 'AUS', C: 'CHN' };
const BY_TAG = {
  USA: 'USA',
  Europe: 'EUR',
  Japan: 'JPN',
  World: 'World',
  Germany: 'NOE',
  France: 'FRA',
  Spain: 'ESP',
  Italy: 'ITA',
  Netherlands: 'HOL',
  Korea: 'KOR',
  Australia: 'AUS',
  China: 'CHN',
};

export function smallPrint(platform, code, tags) {
  if (platform !== 'GBA') return '';
  if (/^[A-Z0-9]{4}$/.test(code)) {
    const region = BY_LETTER[code[3]];
    return region ? `AGB-${code}-${region}` : `AGB-${code}`;
  }
  const regions = [...new Set(tags.map((tag) => BY_TAG[tag]).filter(Boolean))];
  return regions.length ? `AGB · ${regions.join(', ')}` : 'AGB';
}
