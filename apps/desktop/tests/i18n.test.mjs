import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import ts from 'typescript';

// Exercise the production TypeScript without adding a test runner or a runtime dependency.
const source = fs.readFileSync(new URL('../src/i18n.ts', import.meta.url), 'utf8');
const catalog = JSON.parse(fs.readFileSync(new URL('../src/locales/pt-BR.json', import.meta.url), 'utf8'));
const compiled = ts.transpileModule(source, { compilerOptions: {
  module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true,
} }).outputText;
const exports = {};
const document = { documentElement: { lang: '' } };
new Function('require', 'exports', 'navigator', 'document', compiled)(() => catalog, exports, { languages: ['en-US'] }, document);
const { resolveLocale, translate, setLanguage, currentLocale, subscribeLocale, detail, decimal } = exports;

test('explicit preference, system detection, locale spelling and English fallback', () => {
  assert.equal(resolveLocale('auto', ['pt-BR', 'en-US']), 'pt-BR');
  assert.equal(resolveLocale('auto', ['pt_BR']), 'pt-BR');
  assert.equal(resolveLocale('auto', ['pt-PT']), 'pt-BR');
  assert.equal(resolveLocale('auto', ['fr-FR', 'pt-BR']), 'en');
  assert.equal(resolveLocale('auto', []), 'en');
  assert.equal(resolveLocale('en', ['pt-BR']), 'en');
  assert.equal(resolveLocale('pt-BR', ['en-US']), 'pt-BR');
});

test('every catalog entry is nonempty and preserves named placeholders', () => {
  const parameters = (text) => [...text.matchAll(/\{\w+\}/g)].map((match) => match[0]).sort();
  for (const [key, translation] of Object.entries(catalog)) {
    assert.ok(translation.trim(), key);
    assert.deepEqual(parameters(translation), parameters(key), key);
    assert.equal(translate('en', key), key, `English fallback: ${key}`);
  }
  assert.equal(translate('pt-BR', '{count} games', { count: 42 }), '42 partidas');
});

test('language changes notify every subscriber and update document accessibility language', () => {
  let updates = 0;
  const stop = subscribeLocale(() => updates++);
  setLanguage('pt-BR');
  assert.equal(currentLocale(), 'pt-BR');
  assert.equal(document.documentElement.lang, 'pt-BR');
  assert.equal(updates, 1);
  setLanguage('pt-BR');
  assert.equal(updates, 1);
  stop();
  setLanguage('en');
  assert.equal(updates, 1);
});

test('backend diagnostics translate complete messages and retain external evidence', () => {
  setLanguage('pt-BR');
  assert.equal(detail('Could not check for updates: HTTP 503'), 'Não foi possível verificar atualizações: HTTP 503');
  assert.equal(detail('champion 157 ready: 12 pools and the build, from the statistics of 2026-11-01'),
    'campeão 157 pronto: 12 grupos e a build, das estatísticas de 2026-11-01');
  assert.equal(detail('champ select: swap failed: the League client was not found'),
    'seleção de campeões: falha na troca: o cliente do League of Legends não foi encontrado');
  assert.equal(detail('a third-party error: C:\\Riot Games\\file.json'), 'a third-party error: C:\\Riot Games\\file.json');
  setLanguage('en');
  assert.equal(detail('Could not check for updates: HTTP 503'), 'Could not check for updates: HTTP 503');
});

test('numbers use Brazilian decimal commas and preserve existing English formatting', () => {
  setLanguage('pt-BR');
  assert.equal(decimal(52.3, 1), '52,3');
  assert.equal(decimal(-1.5, 1), '-1,5');
  setLanguage('en');
  assert.equal(decimal(52.3, 1), '52.3');
});

test('a language event wins over a stale initial config response and unmount removes the listener', async () => {
  let effect, handler, resolveConfig;
  const changes = [];
  let stopped = 0;
  const config = new Promise((resolve) => { resolveConfig = resolve; });
  const hookSource = fs.readFileSync(new URL('../src/language.ts', import.meta.url), 'utf8');
  const hookCode = ts.transpileModule(hookSource, { compilerOptions: {
    module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022,
  } }).outputText;
  const modules = {
    react: { useEffect: (fn) => { effect = fn; }, useSyncExternalStore: (_, snapshot) => snapshot() },
    '@tauri-apps/api/core': { invoke: () => config },
    '@tauri-apps/api/event': { listen: async (_, fn) => { handler = fn; return () => stopped++; } },
    './i18n': { setLanguage: (value) => changes.push(value), currentLocale: () => 'en', subscribeLocale: () => {} },
  };
  const hooks = {};
  new Function('require', 'exports', hookCode)((name) => modules[name], hooks);
  hooks.useLanguage();
  const cleanup = effect();
  await Promise.resolve(); // Let get_config start before the newer backend event arrives.
  handler({ payload: 'pt-BR' });
  resolveConfig({ language: 'en' });
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(changes, ['pt-BR']);
  cleanup();
  await Promise.resolve();
  assert.equal(stopped, 1);
  handler({ payload: 'en' });
  assert.deepEqual(changes, ['pt-BR']);
});

test('visible literal copy uses the catalog, apart from names and diagnostic identifiers', () => {
  const allowed = new Set(['ARAM Mayhem Tracker', 'League of Legends', 'English', 'Português (Brasil)', 'tuning.json']);
  const root = new URL('../src/', import.meta.url);
  for (const path of ['main.tsx', ...fs.readdirSync(new URL('views/', root), { recursive: true }).filter((path) => path.endsWith('.tsx')).map((path) => `views/${path.replaceAll('\\', '/')}`)]) {
    const content = fs.readFileSync(new URL(path, root), 'utf8');
    const tree = ts.createSourceFile(path, content, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
    function walk(node) {
      if (ts.isJsxText(node)) {
        const text = node.getFullText(tree).trim().replace(/\s+/g, ' ');
        if (/[a-zA-Z]/.test(text)) assert.ok(allowed.has(text), `${path}: untranslated JSX text ${text}`);
      }
      if (ts.isCallExpression(node) && node.expression.getText(tree) === 't' && ts.isStringLiteral(node.arguments[0])) {
        assert.ok(Object.hasOwn(catalog, node.arguments[0].text), `${path}: missing key ${node.arguments[0].text}`);
      }
      ts.forEachChild(node, walk);
    }
    walk(tree);
  }
});
