import { describe, expect, it } from 'vitest';
import {
  HEADER,
  NPM_SECTION,
  NoticesError,
  RUST_SECTION,
  SEPARATOR,
  SUPPLEMENTS,
  compareCodePoint,
  diffEntries,
  isNoticeFileName,
  missingSupplementBlocks,
  missingSupplementCrates,
  normalClosure,
  packageKey,
  packageNames,
  parseCargoTree,
  parseNoticeEntries,
  renderNotices,
  renderNpmSection,
  renderSupplement,
  rootCrateKey,
  usesModulePreloadPolyfill,
  usesRolldownRuntime,
  validateNpmEntries,
  viteCoreLicense,
} from './third-party-notices-lib.mjs';

const NORMAL = [{ kind: null, target: null }];
const DEV = [{ kind: 'dev', target: null }];
const BUILD = [{ kind: 'build', target: null }];

function pkg(name, version, dir = `/registry/${name}-${version}`) {
  return { id: `${name} ${version}`, name, version, manifest_path: `${dir}/Cargo.toml` };
}

function node(from, deps) {
  return {
    id: from.id,
    deps: deps.map(([to, depKinds]) => ({ name: to.name, pkg: to.id, dep_kinds: depKinds })),
  };
}

function metadataFixture() {
  const app = pkg('app', '0.1.0', '/work/app');
  const serde = pkg('serde', '1.0.229');
  const serdeDerive = pkg('serde_derive', '1.0.229');
  const itoa = pkg('itoa', '1.0.15');
  const cc = pkg('cc', '1.2.0');
  const shlex = pkg('shlex', '1.3.0');
  const tempfile = pkg('tempfile', '3.20.0');
  const fastrand = pkg('fastrand', '2.3.0');
  const libc = pkg('libc', '0.2.190');
  return {
    packages: [app, serde, serdeDerive, itoa, cc, shlex, tempfile, fastrand, libc],
    resolve: {
      root: app.id,
      nodes: [
        node(app, [
          [serde, NORMAL],
          [cc, BUILD],
          [tempfile, DEV],
          [libc, [...BUILD, ...NORMAL]],
        ]),
        node(serde, [
          [serdeDerive, NORMAL],
          [itoa, NORMAL],
          [app, NORMAL],
        ]),
        node(serdeDerive, []),
        node(itoa, []),
        node(cc, [[shlex, NORMAL]]),
        node(shlex, []),
        node(tempfile, [[fastrand, NORMAL]]),
        node(fastrand, []),
        node(libc, []),
      ],
    },
  };
}

function npmEntry(name, version, identifier = 'MIT', text = `${name} license`) {
  return { name, version, identifier, text };
}

describe('compareCodePoint', () => {
  it('orders by code unit, not by locale', () => {
    expect(['b', 'a', 'B', '@x'].sort(compareCodePoint)).toEqual(['@x', 'B', 'a', 'b']);
    expect(compareCodePoint('same', 'same')).toBe(0);
  });
});

describe('normalClosure', () => {
  it('follows only normal dependency edges and keeps crate directories', () => {
    const closure = normalClosure(metadataFixture());
    expect([...closure.keys()].sort(compareCodePoint)).toEqual([
      'itoa@1.0.15',
      'libc@0.2.190',
      'serde@1.0.229',
      'serde_derive@1.0.229',
    ]);
    expect(closure.get('serde@1.0.229')).toEqual({
      name: 'serde',
      version: '1.0.229',
      dir: '/registry/serde-1.0.229',
    });
  });

  it('excludes the root package even when a dependency points back to it', () => {
    const closure = normalClosure(metadataFixture());
    expect(closure.has('app@0.1.0')).toBe(false);
    expect(rootCrateKey(metadataFixture())).toBe('app@0.1.0');
  });

  it('ignores crates reachable only through dev or build dependencies', () => {
    const closure = normalClosure(metadataFixture());
    for (const key of ['cc@1.2.0', 'shlex@1.3.0', 'tempfile@3.20.0', 'fastrand@2.3.0']) {
      expect(closure.has(key)).toBe(false);
    }
  });

  it('fails when the metadata has no root package', () => {
    const metadata = { ...metadataFixture(), resolve: { root: null, nodes: [] } };
    expect(() => normalClosure(metadata)).toThrow(NoticesError);
    expect(() => rootCrateKey(metadata)).toThrow(NoticesError);
  });
});

describe('parseCargoTree', () => {
  it('reads name and version from every line format cargo tree prints', () => {
    const text = [
      'pulsebar v0.1.0 (/Users/me/pulsebar/src-tauri)',
      'serde v1.0.229',
      '',
      'serde_derive v1.0.229 (proc-macro)',
      'serde v1.0.229 (*)',
      'pre-release v2.0.0-rc.1+build.5',
      '   ',
      '',
    ].join('\n');
    expect([...parseCargoTree(text)]).toEqual([
      'pulsebar@0.1.0',
      'serde@1.0.229',
      'serde_derive@1.0.229',
      'pre-release@2.0.0-rc.1+build.5',
    ]);
  });

  it('returns an empty set for empty output', () => {
    expect(parseCargoTree('').size).toBe(0);
  });

  it('fails on a line without a version', () => {
    expect(() => parseCargoTree('serde 1.0.229')).toThrow(NoticesError);
    expect(() => parseCargoTree('serde')).toThrow(NoticesError);
  });
});

describe('parseNoticeEntries', () => {
  const text = [
    'Third-party notices',
    '[crate] early 1.0.0 https://crates.io/crates/early/1.0.0',
    '[npm] early 1.0.0 MIT',
    RUST_SECTION,
    SEPARATOR,
    'MIT License (MIT)',
    '',
    '[crate] serde 1.0.229 https://crates.io/crates/serde/1.0.229',
    '[crate] my-crate_2 2.0.0-rc.1+meta https://crates.io/crates/my-crate_2/2.0.0-rc.1+meta',
    '',
    'license text mentioning crates',
    '[crate] wrongurl 1.0.0 https://crates.io/crates/other/1.0.0',
    '[crate] wrongversion 1.0.0 https://crates.io/crates/wrongversion/1.0.1',
    '[crate]  spaced 1.0.0 https://crates.io/crates/spaced/1.0.0',
    '[crate] trailing 1.0.0 https://crates.io/crates/trailing/1.0.0 ',
    ' [crate] indented 1.0.0 https://crates.io/crates/indented/1.0.0',
    '[crate] plain 1.0.0 http://crates.io/crates/plain/1.0.0',
    '[npm] in-rust 1.0.0 MIT',
    NPM_SECTION,
    '',
    SEPARATOR,
    '[npm] svelte 5.57.2 MIT',
    '',
    'svelte license text',
    '[crate] in-npm 1.0.0 https://crates.io/crates/in-npm/1.0.0',
    RUST_SECTION,
    '[crate] after-reset 1.0.0 https://crates.io/crates/after-reset/1.0.0',
    '[npm] @tauri-apps/api 2.12.3 Apache-2.0 OR MIT',
    '[npm] no-identifier 1.0.0 ',
    '[npm]  spaced 1.0.0 MIT',
    '',
  ].join('\n');

  it('counts only exact entry lines inside their own section', () => {
    const entries = parseNoticeEntries(text);
    expect([...entries.rust]).toEqual(['serde@1.0.229', 'my-crate_2@2.0.0-rc.1+meta']);
    expect([...entries.npm]).toEqual(['svelte@5.57.2', '@tauri-apps/api@2.12.3']);
  });

  it('accepts CRLF line endings', () => {
    const entries = parseNoticeEntries(text.replaceAll('\n', '\r\n'));
    expect(entries.rust.has('serde@1.0.229')).toBe(true);
    expect(entries.npm.has('svelte@5.57.2')).toBe(true);
  });

  it('finds nothing without the section headers', () => {
    const entries = parseNoticeEntries(
      '[crate] serde 1.0.229 https://crates.io/crates/serde/1.0.229\n[npm] svelte 5.57.2 MIT\n',
    );
    expect(entries.rust.size).toBe(0);
    expect(entries.npm.size).toBe(0);
  });
});

describe('diffEntries', () => {
  const linked = new Set(['libc@0.2.190', 'serde@1.0.229']);
  const closure = new Map([
    ['libc@0.2.190', { dir: '/registry/libc' }],
    ['serde@1.0.229', { dir: '/registry/serde' }],
    ['serde_derive@1.0.229', { dir: '/registry/serde_derive' }],
  ]);

  it('reports a linked crate that is not listed', () => {
    const listed = new Set(['serde@1.0.229']);
    expect(diffEntries({ required: linked, allowed: closure, actual: listed })).toEqual({
      missing: ['libc@0.2.190'],
      extra: [],
    });
  });

  it('reports a stale version that is no longer in the lockfile closure', () => {
    const listed = new Set(['libc@0.2.190', 'serde@1.0.0', 'serde@1.0.229']);
    expect(diffEntries({ required: linked, allowed: closure, actual: listed })).toEqual({
      missing: [],
      extra: ['serde@1.0.0'],
    });
  });

  it('accepts listed proc-macro crates that are in the closure but not linked', () => {
    const listed = new Set(['serde_derive@1.0.229', 'serde@1.0.229', 'libc@0.2.190']);
    expect(diffEntries({ required: linked, allowed: closure, actual: listed })).toEqual({
      missing: [],
      extra: [],
    });
  });

  it('sorts both lists by code point', () => {
    const required = new Set(['b@1', 'Z@1', 'a@1']);
    const actual = new Set(['y@1', 'X@1']);
    expect(diffEntries({ required, allowed: new Set(), actual })).toEqual({
      missing: ['Z@1', 'a@1', 'b@1'],
      extra: ['X@1', 'y@1'],
    });
  });
});

describe('validateNpmEntries', () => {
  it('accepts complete entries', () => {
    expect(
      validateNpmEntries([
        npmEntry('svelte', '5.57.2'),
        npmEntry('a', '1.0.0', 'Apache-2.0 OR MIT'),
      ]),
    ).toEqual([]);
  });

  it('flags missing or empty fields', () => {
    const errors = validateNpmEntries([
      { name: 'clsx', version: '2.1.1', identifier: 'MIT' },
      { name: 'empty', version: '1.0.0', identifier: '  ', text: 'text' },
      null,
    ]);
    expect(errors).toEqual([
      'npm entry #0 clsx@2.1.1 has no text',
      'npm entry #1 empty@1.0.0 has no identifier',
      'npm entry #2 ?@? has no name',
      'npm entry #2 ?@? has no version',
      'npm entry #2 ?@? has no identifier',
      'npm entry #2 ?@? has no text',
    ]);
  });

  it('flags an SPDX document used as license text', () => {
    const errors = validateNpmEntries([
      npmEntry('pkg', '1.0.0', 'MIT', '\n  SPDXVersion: SPDX-2.2\nDataLicense: CC0-1.0'),
    ]);
    expect(errors).toEqual([
      'npm entry #0 pkg@1.0.0 uses an SPDX document instead of a license text',
    ]);
  });

  it('flags values that would not round-trip through the notices file', () => {
    const errors = validateNpmEntries([
      npmEntry('bad name', '1.0.0'),
      npmEntry('pkg', '1.0.0', 'MIT\nOR Apache-2.0'),
    ]);
    expect(errors).toEqual([
      'npm entry #0 bad name@1.0.0 has whitespace in its name or version',
      'npm entry #1 pkg@1.0.0 has a multi-line license identifier',
    ]);
  });

  it('rejects a non-array payload', () => {
    expect(validateNpmEntries({})).toEqual(['licenses.json is not an array']);
  });
});

describe('bundle runtime detection', () => {
  it('detects the modulepreload polyfill', () => {
    expect(usesModulePreloadPolyfill(['const a = 1;', 'link.relList.supports'])).toBe(true);
    expect(usesModulePreloadPolyfill(['const a = 1;'])).toBe(false);
  });

  it('detects the __commonJS runtime', () => {
    expect(usesRolldownRuntime(['var x = __commonJS({});'])).toBe(true);
    expect(usesRolldownRuntime(['const a = 1;'])).toBe(false);
  });
});

describe('viteCoreLicense', () => {
  const licenseMd = [
    '# Vite core license',
    'Vite is released under the MIT license:',
    '',
    'MIT License',
    '',
    'Copyright (c) 2019-present, VoidZero Inc. and Vite contributors',
    '',
    'Permission is hereby granted.',
    '',
    '# Licenses of bundled dependencies',
    'The published Vite artifact additionally contains code with the following licenses:',
    '> MIT License',
  ].join('\n');

  it('extracts the core license section', () => {
    expect(viteCoreLicense(licenseMd)).toBe(
      [
        'MIT License',
        '',
        'Copyright (c) 2019-present, VoidZero Inc. and Vite contributors',
        '',
        'Permission is hereby granted.',
      ].join('\n'),
    );
  });

  it('fails when the bundled dependencies heading is missing', () => {
    const truncated = licenseMd.replace('# Licenses of bundled dependencies', '# Other');
    expect(() => viteCoreLicense(truncated)).toThrow(NoticesError);
  });

  it('fails when the MIT License line is missing', () => {
    expect(() => viteCoreLicense(licenseMd.replace('\nMIT License\n', '\nMIT\n'))).toThrow(
      NoticesError,
    );
  });
});

describe('renderNpmSection', () => {
  const entries = [
    npmEntry('alpha', '1.0.0', 'MIT', 'alpha text\n\n'),
    npmEntry('Zeta', '2.0.0', 'ISC', 'zeta text'),
    npmEntry('@scope/x', '1.10.0', 'Apache-2.0 OR MIT', 'x text 1.10'),
    npmEntry('@scope/x', '1.2.0', 'MIT', 'x text 1.2'),
  ];

  it('renders entries sorted by name, then version, by code point', () => {
    expect(renderNpmSection(entries)).toBe(
      [
        NPM_SECTION,
        '',
        SEPARATOR,
        '[npm] @scope/x 1.10.0 Apache-2.0 OR MIT',
        '',
        'x text 1.10',
        '',
        SEPARATOR,
        '[npm] @scope/x 1.2.0 MIT',
        '',
        'x text 1.2',
        '',
        SEPARATOR,
        '[npm] Zeta 2.0.0 ISC',
        '',
        'zeta text',
        '',
        SEPARATOR,
        '[npm] alpha 1.0.0 MIT',
        '',
        'alpha text',
      ].join('\n'),
    );
  });

  it('does not depend on input order or mutate the input', () => {
    const reversed = [...entries].reverse();
    const snapshot = [...reversed];
    expect(renderNpmSection(reversed)).toBe(renderNpmSection(entries));
    expect(reversed).toEqual(snapshot);
  });

  it('renders only the heading when there are no entries', () => {
    expect(renderNpmSection([])).toBe(NPM_SECTION);
  });
});

describe('renderNotices', () => {
  const rust = [
    RUST_SECTION,
    SEPARATOR,
    'MIT License (MIT)',
    '',
    '[crate] serde 1.0.229 https://crates.io/crates/serde/1.0.229',
    '',
    'serde license text',
    '',
    '',
  ].join('\n');
  const npmEntries = [npmEntry('svelte', '5.57.2', 'MIT', 'svelte text\n')];

  it('ends with exactly one newline', () => {
    const text = renderNotices({ header: `${HEADER}\n\n`, rust, npmEntries });
    expect(text.endsWith('svelte text\n')).toBe(true);
    expect(text.endsWith('\n\n')).toBe(false);
  });

  it('adds no trailing whitespace and round-trips through parseNoticeEntries', () => {
    const text = renderNotices({ header: HEADER, rust, npmEntries });
    expect(text.split('\n').filter((line) => /[ \t]$/.test(line))).toEqual([]);
    const entries = parseNoticeEntries(text);
    expect([...entries.rust]).toEqual(['serde@1.0.229']);
    expect([...entries.npm]).toEqual([packageKey('svelte', '5.57.2')]);
  });

  it('separates header, Rust section and npm section with one blank line', () => {
    const text = renderNotices({ header: 'Header', rust, npmEntries: [] });
    expect(text).toBe(['Header', '', rust.trimEnd(), '', NPM_SECTION].join('\n').concat('\n'));
  });

  it('fails when the Rust text does not start with the Rust section line', () => {
    expect(() => renderNotices({ header: HEADER, rust: `\n${rust}`, npmEntries })).toThrow(
      NoticesError,
    );
  });
});

describe('isNoticeFileName', () => {
  it('matches file names starting with notice in any case', () => {
    for (const name of ['NOTICE', 'NOTICE.txt', 'notice.md', 'Notices']) {
      expect(isNoticeFileName(name)).toBe(true);
    }
  });

  it('rejects other file names', () => {
    for (const name of ['LICENSE', 'THIRD_PARTY_NOTICE', 'my-notice', 'README.md']) {
      expect(isNoticeFileName(name)).toBe(false);
    }
  });
});

describe('supplements', () => {
  const supplement = {
    crates: ['objc2-foundation', 'block2', 'objc2', 'objc2-encode'],
    license: 'MIT',
    heading: 'MIT License (MIT)',
    file: 'src-tauri/licenses/objc2-LICENSE-MIT.txt',
    source:
      'https://github.com/madsmtm/objc2/blob/ee9a7ada2131f5944b8750428e265c15632f2a19/LICENSE-MIT.txt',
    sha256: 'f6a2e783669a0dd7a2c3b8a4f76db40ae49033e66e1bdc30f3aa25fe5577b070',
  };
  const rust = [
    RUST_SECTION,
    SEPARATOR,
    'MIT License (MIT)',
    '',
    '[crate] objc2 0.6.3 https://crates.io/crates/objc2/0.6.3',
    '',
    'objc2 statement',
    '',
  ].join('\n');

  it('declares the objc2 family supplement with its pinned digest', () => {
    expect(SUPPLEMENTS.map((entry) => [...entry.crates].sort(compareCodePoint))).toEqual([
      ['block2', 'objc2', 'objc2-encode', 'objc2-foundation'],
    ]);
    expect(SUPPLEMENTS[0].sha256).toMatch(/^[0-9a-f]{64}$/);
  });

  it('renders a deterministic block with sorted crate names and the source commit', () => {
    expect(renderSupplement(supplement, 'MIT terms  \n\n')).toBe(
      [
        SEPARATOR,
        'MIT License (MIT), supplementary text',
        '',
        '[supplement] block2 objc2 objc2-encode objc2-foundation',
        '',
        'The released versions of these crates ship only the upstream LICENSE.md statement',
        'listed above under "MIT License (MIT)". Upstream added the following MIT license',
        'text to its repository in commit ee9a7ada2131f5944b8750428e265c15632f2a19:',
        'https://github.com/madsmtm/objc2/blob/ee9a7ada2131f5944b8750428e265c15632f2a19/LICENSE-MIT.txt',
        '',
        'MIT terms',
      ].join('\n'),
    );
    expect(supplement.crates[0]).toBe('objc2-foundation');
  });

  it('detects a notices file that lost the supplementary block', () => {
    const rendered = renderSupplement(supplement, 'MIT terms');
    expect(missingSupplementBlocks([supplement], `${rust}\n${rendered}\n`)).toEqual([]);
    expect(missingSupplementBlocks([supplement], rust)).toEqual([supplement]);
    expect(missingSupplementBlocks([supplement], `${rust}\n[supplement] objc2 block2\n`)).toEqual([
      supplement,
    ]);
  });

  it('fails when the source URL names no commit', () => {
    const unpinned = { ...supplement, source: 'https://github.com/madsmtm/objc2/LICENSE-MIT.txt' };
    expect(() => renderSupplement(unpinned, 'MIT terms')).toThrow(NoticesError);
  });

  it('places the block after the Rust text and before the npm section', () => {
    const text = renderNotices({
      header: 'Header',
      rust,
      supplements: [{ supplement, text: 'MIT terms\n' }],
      npmEntries: [npmEntry('svelte', '5.57.2')],
    });
    const rustEnd = text.indexOf('objc2 statement');
    const block = text.indexOf(`\n\n${SEPARATOR}\nMIT License (MIT), supplementary text\n`);
    const npm = text.indexOf(`\n\n${NPM_SECTION}\n`);
    expect(rustEnd).toBeGreaterThan(-1);
    expect(block).toBeGreaterThan(rustEnd);
    expect(npm).toBeGreaterThan(block);
    expect(text.slice(rustEnd, block)).toBe('objc2 statement');
    expect(text.endsWith('svelte license\n')).toBe(true);
  });

  it('is ignored by parseNoticeEntries', () => {
    const text = renderNotices({
      header: 'Header',
      rust,
      supplements: [{ supplement, text: '[crate] fake 1.0.0 https://crates.io/crates/fake/1.0' }],
      npmEntries: [],
    });
    expect([...parseNoticeEntries(text).rust]).toEqual(['objc2@0.6.3']);
  });

  it('reports supplement crates that cargo-about no longer lists', () => {
    const listed = packageNames(
      new Set(['objc2@0.6.3', 'objc2-encode@4.1.0', 'objc2-foundation@0.3.2', 'pre@1.0.0-rc.1+b']),
    );
    expect(listed.has('pre')).toBe(true);
    expect(missingSupplementCrates(supplement, listed)).toEqual(['block2']);
    expect(missingSupplementCrates(supplement, new Set())).toEqual([
      'block2',
      'objc2',
      'objc2-encode',
      'objc2-foundation',
    ]);
    const otherVersions = packageNames([
      'block2@0.5.1',
      'objc2-app-kit@0.3.2',
      'objc2-encode@4.1.0',
      'objc2-foundation@0.2.2',
    ]);
    expect(missingSupplementCrates(supplement, otherVersions)).toEqual(['objc2']);
  });
});
