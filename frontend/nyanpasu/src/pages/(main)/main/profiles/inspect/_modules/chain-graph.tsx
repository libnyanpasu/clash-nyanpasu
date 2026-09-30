import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react'
import {
  partsText,
  profileName,
  stepKind,
  stepSubject,
  type LabelPart,
  type ProfileLookup,
} from '@/components/profile-label'
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { m } from '@/paraglide/messages'
import type { OperatorTag, RuntimeInspectionNode } from '@nyanpasu/interface'

// Mermaid scopes this under the diagram's own id, so the rules win over its
// default theme without leaking out. The tokens follow the app theme live.
const THEME_CSS = `
  /* Mermaid pins a color on every span, so each state sets its text color as
     a variable that every text element reads directly. */
  .node {
    --node-fg: var(--color-on-surface-variant);
    cursor: pointer;
    outline: none;
  }
  .node.changed { --node-fg: var(--color-on-secondary-container); }
  .node.is-selected { --node-fg: var(--color-on-primary); }
  .node .nodeLabel,
  .node .step-head,
  .node .step-head span,
  .node .step-name { color: var(--node-fg); }
  .node rect {
    fill: var(--color-surface-variant);
    stroke: transparent;
    stroke-width: 2px;
    rx: 12px;
    ry: 12px;
    transition: fill 0.2s, stroke 0.2s;
  }
  .node.changed rect { fill: var(--color-secondary-container); }
  .node:hover rect { stroke: var(--color-outline-variant); }
  .node:focus-visible rect { stroke: var(--color-primary); }
  .node.is-selected rect { fill: var(--color-primary); }
  .flowchart-link { stroke: var(--color-outline); }
  .marker { fill: var(--color-outline); stroke: var(--color-outline); }
  .node .profile-tag {
    background: var(--color-tertiary-container);
    color: var(--color-on-tertiary-container);
    border-radius: 6px;
    padding: 1px 6px;
    display: inline-block;
    white-space: nowrap;
  }
  .step-head {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
  }
  .step-head.has-subject { margin-bottom: 4px; font-size: 11px; }
  /* Fade through the color, not opacity: WebKit paints an opacity layer
     inside a foreignObject without the node's transform. */
  .node .step-head.has-subject,
  .node .step-head.has-subject span {
    color: color-mix(in srgb, var(--node-fg) 80%, transparent);
  }
  .step-index {
    box-sizing: border-box;
    min-width: 18px;
    padding: 0 5px;
    border-radius: 9px;
    background: color-mix(in srgb, currentColor 14%, transparent);
    font-family: var(--font-mono);
    font-size: 10px;
    line-height: 18px;
  }
  .step-name { font-weight: 500; }
`

const NODE_ID_PATTERN = /flowchart-n(\d+)-\d+$/

// Ids go into an attribute of the label markup, so only plain ones do.
const TAGGABLE_ID_PATTERN = /^[\w-]+$/

function escapeLabel(label: string): string {
  return label
    .replaceAll('#', '#35;')
    .replaceAll('"', '#quot;')
    .replaceAll('<', '#lt;')
    .replaceAll('>', '#gt;')
}

function subjectMarkup(subject: LabelPart, profiles: ProfileLookup): string {
  if (typeof subject === 'string') {
    return `<span class='step-name'>${escapeLabel(subject)}</span>`
  }
  const name = escapeLabel(profileName(profiles, subject.profileId))
  return TAGGABLE_ID_PATTERN.test(subject.profileId)
    ? `<span class='profile-tag' data-profile-id='${subject.profileId}'>${name}</span>`
    : `<span class='step-name'>${name}</span>`
}

// A chain node names only the step itself; where its result goes is what the
// edges already show.
function nodeMarkup(
  node: RuntimeInspectionNode,
  profiles: ProfileLookup,
): string {
  const subject = stepSubject(node.tag)
  const head =
    `<span class='step-index'>${node.id + 1}</span>` +
    `<span>${escapeLabel(stepKind(node.tag))}</span>`
  return subject === undefined
    ? `<span class='step-head'>${head}</span>`
    : `<span class='step-head has-subject'>${head}</span>` +
        subjectMarkup(subject, profiles)
}

function nodeText(tag: OperatorTag, profiles: ProfileLookup): string {
  const subject = stepSubject(tag)
  return subject === undefined
    ? stepKind(tag)
    : `${stepKind(tag)} · ${partsText([subject], profiles)}`
}

function buildSource(
  nodes: RuntimeInspectionNode[],
  profiles: ProfileLookup,
): string {
  // TODO: mark each block with a sheet that tells whether it is internal
  // logic, a profile-scoped (local) chain, or the global chain.
  // Blocked on the runtime inspection node exposing a scope; until then the
  // step tag alone cannot tell the three apart reliably.
  const lines = ['flowchart TD']
  for (const node of nodes) {
    lines.push(`  n${node.id}["${nodeMarkup(node, profiles)}"]`)
  }
  for (const node of nodes) {
    for (const next of node.next) {
      lines.push(`  n${node.id} --> n${next}`)
    }
  }
  const changed = nodes.filter(
    (node) => node.has_logs || (node.changed_fields?.length ?? 0) > 0,
  )
  if (changed.length > 0) {
    lines.push(
      `  class ${changed.map((node) => `n${node.id}`).join(',')} changed`,
    )
  }
  return lines.join('\n')
}

function nodeIdOf(element: Element): number | undefined {
  const match = NODE_ID_PATTERN.exec(element.id)
  return match ? Number(match[1]) : undefined
}

function profileTagOf(target: EventTarget | null): HTMLElement | null {
  return (target as Element | null)?.closest?.('[data-profile-id]') ?? null
}

export default function ChainGraph({
  nodes,
  profiles,
  selectedId,
  onSelect,
  onOpenProfile,
}: {
  nodes: RuntimeInspectionNode[]
  profiles: ProfileLookup
  selectedId?: number
  onSelect: (id: number) => void
  onOpenProfile: (id: string) => void
}) {
  const diagramId = `inspect-chain-${useId().replace(/[^\w-]/g, '')}`
  const containerRef = useRef<HTMLDivElement>(null)
  const source = buildSource(nodes, profiles)
  const [rendered, setRendered] = useState<{ source: string; svg: string }>()
  const [failed, setFailed] = useState(false)
  // The tags live inside the SVG, so the tooltip anchors to a fixed overlay
  // placed over the hovered one.
  const [hovered, setHovered] = useState<{ id: string; rect: DOMRect }>()

  useEffect(() => {
    let cancelled = false
    const fontFamily = containerRef.current
      ? getComputedStyle(containerRef.current).fontFamily
      : undefined
    import('mermaid')
      .then(({ default: mermaid }) => {
        mermaid.initialize({
          startOnLoad: false,
          securityLevel: 'strict',
          theme: 'base',
          look: 'classic',
          themeVariables: { fontFamily, fontSize: '13px' },
          // Keep the natural size; the fixed viewport scrolls instead.
          flowchart: { useMaxWidth: false },
          themeCSS: THEME_CSS,
        })
        return mermaid.render(diagramId, source)
      })
      .then(({ svg }) => {
        if (!cancelled) {
          setRendered({ source, svg })
          setFailed(false)
        }
      })
      .catch(() => {
        if (!cancelled) setFailed(true)
      })
    return () => {
      cancelled = true
    }
  }, [diagramId, source])

  const svg = rendered?.source === source ? rendered.svg : undefined

  // The rendered nodes are plain SVG groups; expose them as buttons.
  useLayoutEffect(() => {
    for (const element of containerRef.current?.querySelectorAll('g.node') ??
      []) {
      const id = nodeIdOf(element)
      const node = nodes.find((node) => node.id === id)
      if (!node) continue
      element.setAttribute('role', 'button')
      element.setAttribute('tabindex', '0')
      element.setAttribute(
        'aria-label',
        `#${node.id + 1} ${nodeText(node.tag, profiles)}`,
      )
    }
  }, [svg, nodes, profiles])

  useLayoutEffect(() => {
    for (const element of containerRef.current?.querySelectorAll('g.node') ??
      []) {
      const selected = nodeIdOf(element) === selectedId
      element.classList.toggle('is-selected', selected)
      if (selected) {
        element.setAttribute('aria-current', 'step')
      } else {
        element.removeAttribute('aria-current')
      }
    }
  }, [svg, selectedId])

  const selectFrom = (target: EventTarget) => {
    const element = (target as Element).closest?.('g.node')
    const id = element ? nodeIdOf(element) : undefined
    if (id !== undefined) onSelect(id)
  }

  if (failed) {
    return (
      <p role="alert" className="text-on-surface-variant text-sm">
        {m.inspect_chain_error()}
      </p>
    )
  }

  return (
    <div
      className="bg-surface h-[65vh] overflow-auto rounded-lg @[40rem]:h-auto @[40rem]:min-h-0 @[40rem]:flex-1"
      onScroll={() => setHovered(undefined)}
    >
      <div
        ref={containerRef}
        role="group"
        aria-label={m.inspect_steps()}
        aria-busy={!svg}
        className="flex min-h-full min-w-fit items-center justify-center p-3"
        onClick={(event) => selectFrom(event.target)}
        onKeyDown={(event) => {
          if (event.key !== 'Enter' && event.key !== ' ') return
          event.preventDefault()
          selectFrom(event.target)
        }}
        onDoubleClick={(event) => {
          const id = profileTagOf(event.target)?.dataset.profileId
          if (id) onOpenProfile(id)
        }}
        onPointerOver={(event) => {
          const tag = profileTagOf(event.target)
          const id = tag?.dataset.profileId
          if (tag && id) setHovered({ id, rect: tag.getBoundingClientRect() })
        }}
        onPointerOut={(event) => {
          if (profileTagOf(event.relatedTarget) !== profileTagOf(event.target))
            setHovered(undefined)
        }}
        dangerouslySetInnerHTML={svg ? { __html: svg } : undefined}
      />
      {hovered && (
        <Tooltip open>
          <TooltipTrigger asChild>
            <span
              aria-hidden
              className="pointer-events-none fixed"
              style={{
                left: hovered.rect.left,
                top: hovered.rect.top,
                width: hovered.rect.width,
                height: hovered.rect.height,
              }}
            />
          </TooltipTrigger>
          <TooltipContent className="font-mono">{hovered.id}</TooltipContent>
        </Tooltip>
      )}
    </div>
  )
}
