/**
 * Bounded import/path guard, not Rust type resolution or macro expansion.
 * Accept self::* and super::* when the named module is itself policed: imported
 * names inherit that module's bindings, recursively through accepted globs, and
 * resolve at their declaration scope. Other globs remain unsupported, including
 * provider re-export globs.
 */
import { existsSync, readFileSync } from 'node:fs'
import * as path from 'node:path'
import { createLexState, stripCommentsAndLiterals } from './lexer.ts'

type Token = { text: string; line: number }
type Reference = {
  names: string[]
  token: Token
  scope: Scope
  macro?: boolean
}
type Binding =
  | { kind: 'import'; reference: Reference }
  | { kind: 'module'; scope: Scope }
  | { kind: 'local' }
  | { kind: 'macro'; scope: Scope }
  | { kind: 'alias'; references: Reference[] }
type Problem = { token: Token; message: string }
type Scope = {
  file: string
  module: string[]
  parent?: Scope
  moduleScope: boolean
  bindings: Map<string, Binding[]>
  globSources: string[][]
  references: Reference[]
  problems: Problem[]
  typeAlias?: boolean
}

const MAX_ALIAS_HOPS = 8
const IDENTIFIER = /^[A-Za-z_][A-Za-z0-9_]*$/
const PATH_IDENTIFIER = /^(?:r#)?[A-Za-z_][A-Za-z0-9_]*$/
const CONTROL = new Set(['actor_v2', 'clash', 'proxies', 'connections'])
// These expression macros cannot introduce imports. Unknown calls fail closed;
// local definitions are checked for import/module-generating tokens.
const EXPRESSION_MACROS = new Set([
  'assert',
  'assert_eq',
  'assert_ne',
  'debug_assert',
  'debug_assert_eq',
  'format',
  'matches',
  'panic',
  'println',
  'unimplemented',
  'unreachable',
  'vec',
  'write',
  'include_str',
  'include_bytes',
  'anyhow::anyhow',
  'anyhow::bail',
  'anyhow::ensure',
  'nanoid::nanoid',
  'serde_json::json',
  'snafu::ensure',
  'tokio::select',
  'tokio::try_join',
  'tracing::debug',
  'tracing::error',
  'tracing::info',
  'tracing::warn',
])

function policy(scope: Scope): 'control' | 'state' | undefined {
  if (CONTROL.has(scope.module[0])) return 'control'
  if (scope.module[0] === 'state') return 'state'
}

function tokens(source: string): Token[] {
  const state = createLexState()
  return source
    .split('\n')
    .flatMap((line, index) =>
      [
        ...stripCommentsAndLiterals(line, state).matchAll(
          /::|(?:r#)?[A-Za-z_][A-Za-z0-9_]*|[^\s]/g,
        ),
      ].map((match) => ({ text: match[0], line: index + 1 })),
    )
}

/** Paths are relative to core's src directory; fixtures use the same reader. */
export function coreModuleViolations(
  readSource: (file: string) => string | undefined,
): string[] {
  const scopes: Scope[] = []
  const modules = new Map<string, Scope[]>()
  const externAliases = new Map<string, { scope: Scope; token: Token }>()
  const problems = new Set<string>()
  const report = (scope: Scope, token: Token, message: string) => {
    problems.add(
      `${scope.file}:${token.line}: ${message}; use explicit imports and ordinary module paths`,
    )
  }
  const createScope = (
    file: string,
    module: string[],
    moduleScope: boolean,
    parent?: Scope,
  ): Scope => {
    const scope: Scope = {
      file,
      module,
      moduleScope,
      parent,
      bindings: new Map(),
      globSources: [],
      references: [],
      problems: [],
    }
    scopes.push(scope)
    if (moduleScope) {
      const key = module.join('::')
      modules.set(key, [...(modules.get(key) ?? []), scope])
    }
    return scope
  }
  const bind = (scope: Scope, name: string, binding: Binding) => {
    scope.bindings.set(name, [...(scope.bindings.get(name) ?? []), binding])
  }
  const unsupported = (scope: Scope, token: Token, message: string) => {
    scope.problems.push({ token, message })
  }

  function parseFile(file: string, module: string[], parent?: Scope): Scope {
    const scope = createScope(file, module, true, parent)
    const source = readSource(file)
    if (source === undefined) {
      unsupported(
        scope,
        { text: '', line: 1 },
        file === 'lib.rs'
          ? 'cannot locate core lib.rs'
          : 'cannot locate same-crate provider module',
      )
    } else {
      parse(
        tokens(source),
        scope,
        path.posix.join(
          path.posix.dirname(file),
          path.posix.basename(file) === 'mod.rs' || file === 'lib.rs'
            ? ''
            : path.posix.basename(file, '.rs'),
        ),
      )
    }
    return scope
  }

  function parse(
    input: Token[],
    initialScope: Scope,
    moduleDirectory: string,
  ): void {
    let i = 0
    const at = () => input[i]?.text
    function useTree(scope: Scope, prefix: string[] = []): void {
      const start = input[i]
      if (at() === '{') {
        i++
        while (i < input.length && at() !== '}') {
          useTree(scope, prefix)
          if (at() !== ',') break
          i++
        }
        if (at() !== '}') throw new Error('unsupported use tree')
        i++
        return
      }
      const names = [...prefix]
      if (at() === '::') {
        if (prefix.length) throw new Error('unsupported absolute use path')
        names.push('')
        i++
      }
      while (PATH_IDENTIFIER.test(at() ?? '')) {
        if (at().startsWith('r#'))
          throw new Error('unsupported raw identifier path')
        names.push(at())
        i++
        if (at() !== '::') break
        i++
        if (at() === '{' || at() === '*') {
          useTree(scope, names)
          return
        }
      }
      if (at() === '*') {
        if (
          policy(scope) &&
          (names.join('::') === 'self' ||
            (names.join('::') === 'super' && scope.module.length > 1))
        ) {
          scope.globSources.push(
            names[0] === 'self' ? scope.module : scope.module.slice(0, -1),
          )
          i++
          return
        }
        throw new Error('glob imports are unsupported')
      }
      if (names.length === prefix.length)
        throw new Error('unsupported use path')
      if (names.at(-1) === 'self' && names.length > 1) names.pop()
      let name = names.at(-1)!
      if (at() === 'as') {
        i++
        if (at()?.startsWith('r#'))
          throw new Error('unsupported raw identifier path')
        if (!IDENTIFIER.test(at() ?? ''))
          throw new Error('unsupported import alias')
        name = at()
        i++
      }
      const ref: Reference = { scope, names, token: start }
      scope.references.push(ref)
      if (name !== '_') bind(scope, name, { kind: 'import', reference: ref })
    }

    function body(
      scope: Scope,
      directory: string,
      end?: string,
      associated = false,
    ): void {
      while (i < input.length && at() !== end) {
        const token = input[i]
        if (at() === '#' && input[i + 1]?.text === '[') {
          i += 2
          let depth = 1
          const attribute: Token[] = []
          while (i < input.length && depth) {
            const next = input[i++]
            if (next.text === '[') depth++
            if (next.text === ']') depth--
            attribute.push(next)
          }
          if (
            attribute.some(
              (part, index) =>
                part.text === 'path' && attribute[index + 1]?.text === '=',
            )
          )
            unsupported(scope, token, 'unsupported #[path] module location')
          continue
        }
        if (at() === 'use') {
          i++
          try {
            useTree(scope)
            if (at() !== ';') throw new Error('unsupported use syntax')
          } catch (error) {
            unsupported(scope, token, (error as Error).message)
            while (i < input.length && at() !== ';') i++
          }
          i++
          continue
        }
        if (at() === 'extern' && input[i + 1]?.text === 'crate') {
          unsupported(scope, token, 'unsupported extern crate alias')
          const alias =
            input[i + 3]?.text === 'as'
              ? input[i + 4]?.text
              : input[i + 2]?.text
          if (alias) externAliases.set(alias, { scope, token })
        }
        if (at() === 'mod' && IDENTIFIER.test(input[i + 1]?.text ?? '')) {
          const name = input[i + 1].text
          i += 2
          const module = [...scope.module, name]
          let child: Scope
          if (at() === ';') {
            const candidates = [
              `${directory}/${name}.rs`,
              `${directory}/${name}/mod.rs`,
            ].map((file) => file.replace(/^\.\//, ''))
            const found = candidates.filter(
              (file) => readSource(file) !== undefined,
            )
            if (found.length !== 1)
              unsupported(
                scope,
                token,
                `${found.length ? 'ambiguous' : 'cannot locate'} same-crate provider ${name}`,
              )
            child = parseFile(found[0] ?? candidates[0], module, scope)
            i++
          } else if (at() === '{') {
            child = createScope(scope.file, module, true, scope)
            i++
            body(child, `${directory}/${name}`, '}')
            i++
          } else {
            unsupported(scope, token, 'unsupported module declaration')
            continue
          }
          bind(scope, name, { kind: 'module', scope: child })
          continue
        }
        let macroEnd = i
        while (
          input[macroEnd + 1]?.text === '::' &&
          IDENTIFIER.test(input[macroEnd + 2]?.text ?? '')
        )
          macroEnd += 2
        if (
          at() === 'macro_rules' ||
          (IDENTIFIER.test(at() ?? '') &&
            input[macroEnd + 1]?.text === '!' &&
            ['(', '{', '['].includes(input[macroEnd + 2]?.text))
        ) {
          const definition = at() === 'macro_rules'
          const name = definition ? input[i + 2]?.text : input[macroEnd].text
          if (definition) bind(scope, name, { kind: 'macro', scope })
          else
            scope.references.push({
              scope,
              token,
              names: input
                .slice(i, macroEnd + 1)
                .filter((part) => part.text !== '::')
                .map((part) => part.text),
              macro: true,
            })
          i = definition ? i + 3 : macroEnd + 2
          const open = at()
          const close = open === '{' ? '}' : open === '(' ? ')' : ']'
          let depth = 1
          i++
          const contents: Token[] = []
          while (i < input.length && depth) {
            const next = input[i++]
            if (next.text === open) depth++
            if (next.text === close) depth--
            contents.push(next)
          }
          if (
            contents.some((part) =>
              ['use', 'mod', 'extern', 'type'].includes(part.text),
            ) ||
            name === 'include' ||
            (!definition &&
              scope.moduleScope &&
              !['include_str', 'include_bytes'].includes(name))
          ) {
            unsupported(
              scope,
              token,
              'unsupported macro-generated imports or modules',
            )
          }
          // Ordinary expression macro arguments still contain checked paths.
          scanReferences(contents, scope)
          continue
        }
        if (
          [
            'struct',
            'enum',
            'trait',
            'fn',
            'const',
            'static',
            'union',
            'type',
          ].includes(at()) &&
          IDENTIFIER.test(input[i + 1]?.text ?? '')
        ) {
          const name = input[i + 1].text
          if (at() === 'type' && !associated) {
            const rhs: Token[] = []
            i += 2
            while (i < input.length && at() !== '=' && at() !== ';') i++
            if (at() === '=') {
              i++
              while (i < input.length && at() !== ';') rhs.push(input[i++])
            }
            const aliasScope = createScope(
              scope.file,
              scope.module,
              false,
              scope,
            )
            aliasScope.typeAlias = true
            scanReferences(rhs, aliasScope, true)
            bind(scope, name, {
              kind: 'alias',
              references: aliasScope.references,
            })
            i++
            continue
          }
          bind(scope, name, { kind: 'local' })
        }
        const associatedBody = at() === 'impl' || at() === 'trait'
        if (associatedBody) {
          // Associated types are definitions, not module aliases.
          while (i < input.length && at() !== '{' && at() !== ';') {
            i = scanPath(input, scope, i)
          }
        }
        if (at() === '{') {
          i++
          body(
            createScope(scope.file, scope.module, false, scope),
            directory,
            '}',
            associatedBody,
          )
          i++
          continue
        }
        i = scanPath(input, scope, i)
      }
    }

    body(initialScope, moduleDirectory)
  }

  // One path scanner serves ordinary syntax, alias RHSs and macro arguments.
  function scanPath(
    input: Token[],
    scope: Scope,
    start: number,
    bare = false,
  ): number {
    let i = start
    if (input[i].text === '>' && input[i + 1]?.text === '::')
      unsupported(scope, input[i], 'unsupported qualified associated path')
    const names: string[] = []
    if (input[i]?.text === '::') {
      names.push('')
      i++
    }
    if (PATH_IDENTIFIER.test(input[i]?.text ?? '')) {
      names.push(input[i++].text)
      while (
        input[i]?.text === '::' &&
        PATH_IDENTIFIER.test(input[i + 1]?.text ?? '')
      ) {
        names.push(input[i + 1].text)
        i += 2
      }
    }
    const macro =
      input[i]?.text === '!' && ['(', '[', '{'].includes(input[i + 1]?.text)
    if (names.length && (bare || names.length > 1 || macro)) {
      if (names.some((name) => name.startsWith('r#')))
        unsupported(scope, input[start], 'unsupported raw identifier path')
      else scope.references.push({ scope, names, token: input[start], macro })
    }
    return Math.max(i, start + 1)
  }

  function scanReferences(input: Token[], scope: Scope, bare = false): void {
    for (let i = 0; i < input.length;) i = scanPath(input, scope, i, bare)
  }

  const root = parseFile('lib.rs', [])
  for (const problem of root.problems)
    report(root, problem.token, problem.message)

  function resolve(
    ref: Reference,
    consumer: Scope,
    hops = 0,
    seen = new Set<Binding>(),
  ): void {
    const fail = (message: string) => report(ref.scope, ref.token, message)
    const check = (module: string[], name?: string) => {
      if (
        (policy(consumer) === 'control' && module[0] === 'client') ||
        (policy(consumer) === 'state' &&
          ((module[0] === 'client' && module[1] === 'application_workflow') ||
            // Match by name on purpose, mirroring the spec's rg reverse check.
            name === 'ApplicationWorkflowClient'))
      ) {
        fail(
          `${ref.scope.typeAlias ? 'unsupported type alias hides ' : ''}forbidden ${policy(consumer)} import ${[...module, name].filter(Boolean).join('::')}`,
        )
      }
    }
    const inspect = (scope: Scope) => {
      for (const problem of scope.problems)
        report(scope, problem.token, problem.message)
    }
    const findBindings = (
      scope: Scope,
      name: string,
      visited = new Set<Scope>(),
    ): Binding[] => {
      if (visited.has(scope)) return []
      const bindings = scope.bindings.get(name)
      if (bindings) return bindings
      const nextVisited = new Set(visited).add(scope)
      const inherited = scope.globSources.flatMap((module) => {
        const candidates = modules.get(module.join('::')) ?? []
        if (candidates.length !== 1) {
          fail('ambiguous or missing relative provider')
          return []
        }
        return findBindings(candidates[0], name, nextVisited)
      })
      return [...new Set(inherited)]
    }
    const expand = (binding: Binding, suffix: string[]) => {
      if (seen.has(binding)) {
        fail('cyclic import/re-export')
        return
      }
      const nextSeen = new Set(seen).add(binding)
      if (
        (binding.kind === 'import' || binding.kind === 'alias') &&
        hops >= MAX_ALIAS_HOPS
      ) {
        fail('import/re-export or type alias exceeds 8 hops')
        return
      }
      if (binding.kind === 'module') {
        walk(binding.scope, suffix)
      } else if (binding.kind === 'import') {
        resolve(
          {
            ...binding.reference,
            names: [...binding.reference.names, ...suffix],
            macro: ref.macro,
          },
          consumer,
          hops + 1,
          nextSeen,
        )
      } else if (binding.kind === 'macro') {
        if (!policy(binding.scope)) fail('unsupported provider macro expansion')
      } else if (binding.kind === 'alias') {
        // Inspect only the alias syntax, never fields, signatures or function bodies.
        for (const target of binding.references) {
          inspect(target.scope)
          resolve(target, consumer, hops + 1, nextSeen)
        }
      }
    }
    const walk = (scope: Scope, names: string[]) => {
      check(scope.module, names[0])
      inspect(scope)
      if (!names.length) return
      const bindings = findBindings(scope, names[0])
      // The provider module is known. Terminal items may be derive-generated
      // (for example Snafu selectors); this guard does not resolve Rust items.
      if (
        !bindings.length &&
        names.length === 1 &&
        scope.module.length &&
        !ref.macro
      )
        return
      if (bindings.length !== 1) {
        fail(
          `${bindings.length ? 'ambiguous' : 'cannot locate'} same-crate provider ${[...scope.module, names[0]].join('::')}`,
        )
        return
      }
      expand(bindings[0], names.slice(1))
    }

    const names = [...ref.names]
    let scope = ref.scope
    const externalName = names[0] === '' ? names[1] : names[0]
    const externalAlias = externAliases.get(externalName)
    if (externalAlias) {
      report(
        externalAlias.scope,
        externalAlias.token,
        'unsupported extern crate alias',
      )
      return
    }
    if (names[0] === '') {
      if (ref.macro && !EXPRESSION_MACROS.has(names.slice(1).join('::')))
        fail('unsupported macro expansion')
      return // Rust 2018 absolute paths name external crates.
    }
    if (names[0] === 'crate') {
      names.shift()
      walk(root, names)
      return
    }
    if (names[0] === 'self' || names[0] === 'super') {
      const module = [...scope.module]
      if (names[0] === 'self') names.shift()
      while (names[0] === 'super') {
        names.shift()
        if (!module.length) {
          fail('cannot locate super provider')
          return
        }
        module.pop()
      }
      const candidates = modules.get(module.join('::')) ?? []
      if (candidates.length !== 1) {
        fail('ambiguous or missing relative provider')
        return
      }
      walk(candidates[0], names)
      return
    }
    while (true) {
      const bindings = findBindings(scope, names[0])
      if (bindings.length) {
        if (bindings.length !== 1) {
          fail(`ambiguous import ${names[0]}`)
          return
        }
        expand(bindings[0], names.slice(1))
        return
      }
      if (scope.moduleScope || !scope.parent) break
      scope = scope.parent
    }
    // Unbound ordinary roots are external crates or local Rust values/types.
    if (ref.macro && !EXPRESSION_MACROS.has(names.join('::')))
      fail('unsupported macro expansion')
  }

  for (const scope of scopes) {
    if (!policy(scope)) continue
    for (const problem of scope.problems)
      report(scope, problem.token, problem.message)
    for (const ref of scope.references) resolve(ref, scope)
  }
  return [...problems].sort()
}

export function checkCoreModuleBoundaries(sourceRoot: string): string[] {
  return coreModuleViolations((file) => {
    const absolute = path.join(sourceRoot, file)
    return existsSync(absolute) ? readFileSync(absolute, 'utf8') : undefined
  })
}
