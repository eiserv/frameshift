export const metadata = { title: 'Statements' };

export default function StatementsPage() {
  const items = ['first', 'second'];
  // An expression statement keeps its place before the markup.
  items.reverse();
  return (
    <ul>
      {items.map((item) => (
        <li key={item}>{`${item}`}</li>
      ))}
    </ul>
  );
}
