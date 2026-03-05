import { useEffect, useRef, useState } from 'react';

/**
 * Scroll-triggered reveal. Content is ALWAYS visible (isVisible starts true
 * for SSR/no-JS). The hook flips to false on mount, then back to true when
 * the element scrolls into view — giving a subtle entrance animation as
 * progressive enhancement only.
 */
export function useScrollReveal<T extends HTMLElement = HTMLDivElement>(
  threshold = 0.1,
): [React.RefObject<T | null>, boolean] {
  const ref = useRef<T | null>(null);
  // Start true so SSR and no-JS always show content
  const [isVisible, setIsVisible] = useState(true);

  useEffect(() => {
    if (typeof window === 'undefined') return;
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    if (typeof IntersectionObserver === 'undefined') return;

    const el = ref.current;
    if (!el) return;

    // Only hide if the element is NOT already in the viewport
    const rect = el.getBoundingClientRect();
    const inViewport = rect.top < window.innerHeight && rect.bottom > 0;
    if (inViewport) {
      // Already visible — don't flash
      return;
    }

    // Element is below fold — set up the reveal
    setIsVisible(false);

    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) {
          setIsVisible(true);
          observer.disconnect();
        }
      },
      { threshold },
    );

    observer.observe(el);
    return () => observer.disconnect();
  }, [threshold]);

  return [ref, isVisible];
}
