export default function HighlightText({
  searchText,
  className,
  children,
}: {
  searchText: string
  className?: string
  children: string
}) {
  if (!searchText.trim()) {
    return <span className={className}>{children}</span>
  }

  const parts: { text: string; isHighlight: boolean }[] = []
  const searchLower = searchText.toLowerCase()
  const textLower = children.toLowerCase()

  let lastIndex = 0
  let index = textLower.indexOf(searchLower, lastIndex)

  while (index !== -1) {
    // Add text before match
    if (index > lastIndex) {
      parts.push({
        text: children.slice(lastIndex, index),
        isHighlight: false,
      })
    }

    // Add matched text
    parts.push({
      text: children.slice(index, index + searchText.length),
      isHighlight: true,
    })

    lastIndex = index + searchText.length
    index = textLower.indexOf(searchLower, lastIndex)
  }

  // Add remaining text
  if (lastIndex < children.length) {
    parts.push({
      text: children.slice(lastIndex),
      isHighlight: false,
    })
  }

  return (
    <span className={className}>
      {parts.map((part, index) =>
        part.isHighlight ? (
          <mark
            key={index}
            className="rounded bg-yellow-400 px-0.5 text-black dark:bg-yellow-500"
          >
            {part.text}
          </mark>
        ) : (
          <span key={index}>{part.text}</span>
        ),
      )}
    </span>
  )
}
