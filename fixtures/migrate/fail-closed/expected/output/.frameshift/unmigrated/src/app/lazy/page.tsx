const loadGreeting = () => import('../../lib/greeting');

export default function LazyPage() {
  return <p>{String(loadGreeting)}</p>;
}
