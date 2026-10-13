// Writes what src/dict needs of Yomitan as data: the Japanese transforms and
// the character tables of its text preprocessors (src/dict/japanese.json),
// and what Yomitan itself makes of some sample text, for the tests to hold
// the Rust port against (src/dict/vectors.json).
//
// Made from Yomitan 26.9.8.0 (https://github.com/yomidevs/yomitan, the `ext`
// directory, or an unpacked copy of the extension). To make them anew:
//
//   echo '{"type":"module"}' > <yomitan ext>/package.json
//   node tools/yomitan-dump.mjs <yomitan ext> src/dict

import {writeFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';

const [ext, out] = process.argv.slice(2);
if (!ext || !out) {
    console.error('usage: node yomitan-dump.mjs <yomitan ext directory> <output directory>');
    process.exit(1);
}
const load = (file) => import(pathToFileURL(resolve(ext, file)).href);

const {japaneseTransforms} = await load('js/language/ja/japanese-transforms.js');
const {LanguageTransformer} = await load('js/language/language-transformer.js');
const japanese = await load('js/language/ja/japanese.js');
const processors = await load('js/language/ja/japanese-text-preprocessors.js');
const {normalizeRadicalCharacters} = await load('js/language/CJK-util.js');
const {ROMAJI_TO_HIRAGANA} = await load('js/language/ja/japanese-kana-romaji-dicts.js');
const {convertAlphabeticToKana} = await load('js/language/ja/japanese-wanakana.js');

// A rule's test is a regular expression made of its suffix or word; the port
// compares plain text, so nothing in it may mean anything to an expression.
const plain = (text, where) => {
    if (text === '' || /[\\^$.*+?()[\]{}|]/.test(text)) { throw new Error(`not plain text in ${where}: ${text}`); }
    return text;
};
const transforms = Object.entries(japaneseTransforms.transforms).map(([id, {name, rules}]) => ({
    id,
    name,
    rules: rules.map((rule, nth) => {
        const where = `${id}[${nth}]`;
        const source = rule.isInflected.source;
        let inflected;
        let deinflected;
        if (rule.type === 'suffix') {
            if (!source.endsWith('$')) { throw new Error(`no suffix in ${where}`); }
            inflected = plain(source.slice(0, -1), where);
            deinflected = rule.deinflected;
        } else if (rule.type === 'wholeWord') {
            if (!source.startsWith('^') || !source.endsWith('$')) { throw new Error(`no word in ${where}`); }
            inflected = plain(source.slice(1, -1), where);
            deinflected = rule.deinflect(inflected);
        } else {
            throw new Error(`a ${rule.type} rule in ${where}`);
        }
        if (rule.deinflect(`語${inflected}`) !== (rule.type === 'suffix' ? `語${deinflected}` : deinflected)) { throw new Error(`${where} does more than its text says`); }
        return {type: rule.type, inflected, deinflected, in: rule.conditionsIn, out: rule.conditionsOut};
    }),
}));
// in the order Yomitan gives them their flags
const conditions = Object.entries(japaneseTransforms.conditions).map(([name, {isDictionaryForm, subConditions}]) => ({name, dictionary: isDictionaryForm, sub: subConditions ?? null}));

// The tables, read off what Yomitan's functions do to each character.
const characters = function* (from, to) {
    for (let c = from; c <= to; ++c) {
        if (c < 0xd800 || c > 0xdfff) { yield String.fromCodePoint(c); }
    }
};
const changed = (from, to, change) => {
    const table = {};
    for (const c of characters(from, to)) {
        const made = change(c);
        if (made !== c) { table[c] = made; }
    }
    return table;
};
const halfwidth = {};
for (const c of characters(0xff61, 0xff9f)) {
    const plainly = japanese.convertHalfWidthKanaToFullWidth(c);
    if (plainly === c) { continue; }
    const marked = (mark) => {
        const made = japanese.convertHalfWidthKanaToFullWidth(c + mark);
        return made.length === 1 ? made : null;
    };
    halfwidth[c] = [plainly, marked('ﾞ'), marked('ﾟ')];
}
const combined = (mark) => {
    const table = {};
    for (const c of characters(0x3040, 0x30ff)) {
        const made = japanese.normalizeCombiningCharacters(c + mark);
        if (made.length === 1) { table[c] = made; }
    }
    return table;
};
// what a long mark becomes after a character, which is one that the
// conversion to hiragana leaves as it is
const prolonged = {};
for (const c of characters(0, 0xffff)) {
    if (japanese.convertKatakanaToHiragana(c) !== c) { continue; }
    const made = japanese.convertKatakanaToHiragana(`${c}ー`);
    if (made !== `${c}ー`) { prolonged[c] = made.slice(c.length); }
}
const tables = {
    halfwidth,
    voiced: combined('゙'),
    half_voiced: combined('゚'),
    prolonged,
    compatibility: changed(0, 0xffff, japanese.normalizeCJKCompatibilityCharacters),
    radicals: changed(0, 0x3ffff, (c) => normalizeRadicalCharacters.process(c)[1]),
    kanji: changed(0, 0x3ffff, (c) => processors.standardizeKanji.process(c)[1]),
};
// replaced one after the other, so in Yomitan's order; nothing in them may mean anything to a replacement
const romaji = Object.entries(ROMAJI_TO_HIRAGANA);
for (const [, kana] of romaji) {
    if (kana.includes('$')) { throw new Error(`not plain text for romaji: ${kana}`); }
}
writeFileSync(resolve(out, 'japanese.json'), JSON.stringify({conditions, transforms, tables, romaji}));

// What Yomitan makes of sample text. The preprocessors are those of its
// Japanese descriptor, in their order.
const order = ['convertHalfWidthCharacters', 'alphabeticToHiragana', 'normalizeCombiningCharacters', 'normalizeCJKCompatibilityCharacters', 'normalizeRadicalCharacters', 'alphanumericWidthVariants', 'convertHiraganaToKatakana', 'collapseEmphaticSequences', 'standardizeKanji'];
const variants = (text) => {
    let found = [text];
    for (const id of order) {
        const {process: change} = id === 'normalizeRadicalCharacters' ? normalizeRadicalCharacters : processors[id];
        const next = [];
        for (const variant of found) {
            for (const made of change(variant)) {
                if (!next.includes(made)) { next.push(made); }
            }
        }
        found = next;
    }
    return found;
};
const transformer = new LanguageTransformer();
transformer.addDescriptor(japaneseTransforms);
const words = [
    '食べる', '食べた', '食べさせられた', '食べさせられたくなかった', '食べられません', '食べちゃった', '食べろ', '食べよう', '食べれば',
    '行った', '行って', '行かなかった', '行きたい', '行きましょう', '行けば', '行こう', '行かせる', '行かれる',
    '高くなかった', '高ければ', '高さ', '高すぎる', '高そう', '高くて', '高かった', '美しく',
    '来なかった', '来た', '来て', '来られる', '来させる', 'こない', 'きた',
    'した', 'して', 'しない', 'させられる', 'される', '勉強した', '勉強しなかった', 'できる',
    '読んでいる', '読んでた', '読んじゃう', '読まれた', '読めない', '読みすぎた', '読もう', '読みなさい',
    '泳いだ', '話した', '待って', '死んだ', '遊んで', '買った', '買わない', '問うた', '請うて',
    'いらっしゃいます', 'なさい', '下さい', 'ございます',
    '見せてあげる', '言っておく', '知らん', '分からねえ', '走っとる', '食べてる', '寝ず', '知らぬ',
    '静かな', '静かに', '猫', 'ねこ', 'する', 'くる', 'ある', 'ない', 'いい', 'よかった', 'ー', '',
];
const samples = ['ﾖﾐﾁｬﾝ', 'ﾊﾞｲｸ', 'ﾊﾟﾝ', 'ドア', 'ぱん', 'すっっごーーい', 'よみちゃん', 'ヨミチャン', 'コーヒー', 'ｙｏｍｉ', 'yomi12', '萬', '㌀', '⼀', '食べる', 'ヴぁ', 'ー', '', 'Tokyo', 'toukyou', 'KYOTO', 'shinjuku', 'Ｓａｋｕｒａ', 'kon-nichiwa', 'matte', 'tttsu', 'nyan', 'n', 'xtu', 'Hello world', '東京tower'];

// The same again for far more text than can be kept whole: of each only a
// number that stands for everything Yomitan made of it.
const sum = (text) => {
    let hash = 0xcbf29ce484222325n;
    for (const byte of new TextEncoder().encode(text)) {
        hash = ((hash ^ BigInt(byte)) * 0x100000001b3n) & 0xffffffffffffffffn;
    }
    return hash.toString(16);
};
// SAMPLES=8 SEED=1 in the environment checks the port against more and other text
const more = Number(process.env.SAMPLES ?? 1);
let seed = Number(process.env.SEED ?? 20261013);
const random = (below) => {
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) % below;
};
const pick = (list) => list[random(list.length)];
const rules = transforms.flatMap((transform) => transform.rules);
const stems = ['食べ', '行', '高', '読', '勉強', '来', '見', '書', '泳', '話', '待', '死', '遊', '買', '美し', 'し', 'い', ''];
const inflected = new Set(words);
// every rule on its own, then rules laid over one another the way speech does
for (const rule of rules) {
    inflected.add(rule.type === 'suffix' ? pick(stems) + rule.inflected : rule.inflected);
}
const dictionary = ['食べる', '行く', '高い', '読む', '勉強する', '来る', 'くる', '見る', '書く', '泳ぐ', '話す', '待つ', '死ぬ', '遊ぶ', '買う', '美しい', 'する', 'ある', 'いい', '信じる', '論ずる', '問う', 'いらっしゃる', '下さる', '静かだ', '猫'];
for (let n = 0; n < 6000 * more; ++n) {
    let text = pick(dictionary);
    for (let depth = 1 + random(4); depth > 0; --depth) {
        const fitting = rules.filter((rule) => (rule.type === 'suffix' ? text.endsWith(rule.deinflected) : text === rule.deinflected));
        if (fitting.length === 0) { break; }
        const rule = pick(fitting);
        text = rule.type === 'suffix' ? text.slice(0, text.length - rule.deinflected.length) + rule.inflected : rule.inflected;
    }
    inflected.add(text);
}
// and text that is no word, cut from the pieces words are made of
const pieces = [...'あいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほまみむめもやゆよらりるれろわをんっゃゅょがぎぐげござじずぜぞだでどばびぶべぼ', 'ない', 'ます', 'ませ', 'なかった', 'られ', 'させ', 'たい', 'すぎ', 'ている', 'ちゃ', 'ければ', 'くて', 'そう', 'なさい', '食', '行', '高', '来', '為', '居'];
for (let n = 0; n < 3000 * more; ++n) {
    let text = '';
    for (let length = 1 + random(6); length > 0; --length) { text += pick(pieces); }
    inflected.add(text);
}
const covered = new Set();
const formed = [...inflected].map((text) => {
    const forms = transformer.transform(text);
    for (const form of forms) {
        for (const frame of form.trace) { covered.add(`${frame.transform} ${frame.ruleIndex}`); }
    }
    return [text, forms.length, sum(forms.map((form) => `${form.text}\t${form.conditions}\t${form.trace.map((frame) => frame.transform).join(',')}`).join('\n'))];
});
if (covered.size !== rules.length) { throw new Error(`only ${covered.size} of ${rules.length} rules are met in the samples`); }

const letters = [...'ｱｲｳｶｷｸｻｼﾀﾁﾂﾊﾋﾌﾎﾜｦﾝｧｬｯｰﾞﾟ･', ...'あいうえおかきこさしたつてとはひふへほやゆよんっゃゅょぁがぱゔゐゑゖ', ...'アイウエオカキコサシタツテトハヒフヘホヤユヨンッャュョァガパヴヵヶヷー', '゙', '゚', ...'abcdeghijkmnorstuwyzABCDEKNOSTUYZ', ...'ａｂｃｋｎｏｓｔｕＡＫＮＯＳＴＵ', ...'0189０１９-－!！ 　、。', ...'㌀㌔㍍㍿⼀⼈⺟⻳萬國學體澤邊齋櫻', ...'食行高来東京猫', '\u{20b9f}', '\u{1f600}'];
const written = new Set(samples);
for (let n = 0; n < 6000 * more; ++n) {
    let text = '';
    for (let length = 1 + random(8); length > 0; --length) { text += pick(letters); }
    written.add(text);
}
// romaji as names and places are written, where the letters matter most
const syllables = ['a', 'i', 'u', 'e', 'o', 'ka', 'ki', 'ku', 'ke', 'ko', 'sa', 'shi', 'si', 'su', 'se', 'so', 'ta', 'chi', 'ti', 'tsu', 'tu', 'te', 'to', 'na', 'ni', 'nu', 'ne', 'no', 'ha', 'hi', 'fu', 'hu', 'he', 'ho', 'ma', 'mi', 'mu', 'me', 'mo', 'ya', 'yu', 'yo', 'ra', 'ri', 'ru', 're', 'ro', 'wa', 'wo', 'n', 'nn', "n'", 'ga', 'gi', 'gu', 'ge', 'go', 'za', 'ji', 'zi', 'zu', 'ze', 'zo', 'da', 'di', 'du', 'de', 'do', 'ba', 'bi', 'bu', 'be', 'bo', 'pa', 'pi', 'pu', 'pe', 'po', 'kya', 'kyu', 'kyo', 'sha', 'shu', 'sho', 'sya', 'cha', 'chu', 'cho', 'tya', 'nya', 'nyu', 'nyo', 'hya', 'mya', 'rya', 'ryu', 'ryo', 'gya', 'ja', 'ju', 'jo', 'jya', 'bya', 'pya', 'kk', 'ss', 'tt', 'pp', 'cc', 'tch', 'fa', 'fi', 'fe', 'fo', 'va', 'vu', 'la', 'li', 'xa', 'xtu', 'ltu', 'thi', 'dhi', 'wi', 'we', 'q', 'c', 'x', 'l', 'v', '-', 'ou', 'oo', 'uu', 'ei', 'mb', 'mp', 'oh', 'h', 'y', 'w', 't', 's', 'k', 'm', 'r', 'ts', 'sh', 'ch'];
for (let n = 0; n < 6000 * more; ++n) {
    let text = '';
    for (let length = 1 + random(6); length > 0; --length) { text += pick(syllables); }
    const how = random(4);
    written.add(how === 0 ? text.toUpperCase() : how === 1 ? text[0].toUpperCase() + text.slice(1) : how === 2 ? japanese.convertAlphanumericToFullWidth(text) : text);
}
const all = [...written];
const full = (text) => ({
    text,
    kana: convertAlphabeticToKana(text),
    hiragana: japanese.convertKatakanaToHiragana(text),
    katakana: japanese.convertHiraganaToKatakana(text),
    halfwidth: japanese.convertHalfWidthKanaToFullWidth(text),
    combined: japanese.normalizeCombiningCharacters(text),
    narrow: japanese.convertFullWidthAlphanumericToNormal(text),
    wide: japanese.convertAlphanumericToFullWidth(text),
    collapsed: japanese.collapseEmphaticSequences(text, false),
    emptied: japanese.collapseEmphaticSequences(text, true),
    variants: variants(text),
});
const fields = ['kana', 'hiragana', 'katakana', 'halfwidth', 'combined', 'narrow', 'wide', 'collapsed', 'emptied'];
const vectors = {
    transforms: words.map((text) => ({text, forms: transformer.transform(text).map((form) => ({text: form.text, conditions: form.conditions, rules: form.trace.map((frame) => frame.transform)}))})),
    formed,
    parts: ['v1', 'v5', 'vk', 'vs', 'vz', 'adj-i', 'v5 vt', 'v1 vi', 'n', '', 'adj-na', 'v', 'v1d', 'v5 adj-i', 'vs vk'].map((marks) => ({rules: marks, flags: transformer.getConditionFlagsFromPartsOfSpeech(marks.split(' '))})),
    conversions: samples.map(full),
    fields,
    converted: all.map((text) => {
        const made = full(text);
        return [text, sum(fields.map((field) => made[field]).join('\n')), made.variants.length, sum(made.variants.join('\n'))];
    }),
};
writeFileSync(resolve(out, 'vectors.json'), JSON.stringify(vectors));
console.log(`${formed.length} texts deinflected, ${all.length} texts written other ways`);
console.log(`${transforms.length} transforms, ${transforms.reduce((n, t) => n + t.rules.length, 0)} rules, ${conditions.length} conditions`);
for (const [name, table] of Object.entries(tables)) { console.log(`  ${name}: ${Object.keys(table).length}`); }
