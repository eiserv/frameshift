import Counter from '../components/Counter';

export default function HomePage() {
  return (
    <main>
      <h1>Tally board</h1>
      <p>Count the boats that pass the pier.</p>
      <Counter initial={3} />
    </main>
  );
}
