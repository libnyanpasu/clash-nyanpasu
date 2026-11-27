export default function Notice({
  error,
  children,
}: {
  error?: boolean
  children: string
}) {
  return error ? (
    <p
      role="status"
      className="bg-error-container text-on-error-container rounded-large p-4 text-sm"
    >
      {children}
    </p>
  ) : (
    <p className="text-on-surface-variant py-6 text-center text-sm">
      {children}
    </p>
  )
}
