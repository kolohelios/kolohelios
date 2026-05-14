// Career-arc multi-layer chart with collapsible sections sharing
// one time axis. Tufte-style: light strokes, muted fills, every
// pixel earns its place.
//
// Sections (each independently expandable/collapsible):
//   * Seniority — step-function line from Developer (1) through
//     Principal (5).
//   * Companies — wide bands per employer along the time axis.
//   * Industries — softer overlay bands; collapsed by default.
//   * Clients — one row per client engagement; collapsed by
//     default.
//   * Technologies, one section per category — thin "rug" stripes
//     showing when each tech was actively in use.
//
// The chart is wider than its container; the container scrolls
// horizontally. Left-rail labels translate with `scrollLeft` so
// they remain pinned to the visible left edge as the reader pans.
// On first load the container scrolls to its right edge so the
// most recent period is in view.
//
// A vertical hover guide lights up the slice at the cursor's X
// position; the linked #career-arc-detail panel updates to show
// seniority / company / active clients / active tech for that
// month. When the cursor approaches the visible left/right edge,
// the container auto-pans in that direction with a velocity that
// ramps with proximity.
//
// Tech and client rows dim to ~30% opacity when none of their
// date ranges intersect the currently visible window, so the eye
// is guided toward what's relevant for the panned-into era
// without removing rows (which would cause layout shift).

(function () {
  const svg = document.getElementById('career-arc');
  const panel = document.getElementById('career-arc-detail');
  if (!svg || !panel) {
    return;
  }
  const dataUrl = svg.dataset.url || '/data/career-arc.json';
  fetch(dataUrl)
    .then((r) => r.json())
    .then((data) => mount(svg, panel, data))
    .catch((err) => {
      console.error('career-arc: failed to load data:', err);
    });

  function mount(svg, panel, raw) {
    const spanStart = parseMonth(raw.timespan.start);
    const spanEnd = parseMonth(raw.timespan.end);
    const seniority = {
      tracks: raw.seniority.tracks,
      levels: raw.seniority.levels,
      transitions: raw.seniority.transitions
        .map((t) => ({
          date: parseMonth(t.date),
          track: t.track,
          level: t.level,
          note: t.note || '',
        }))
        .sort((a, b) => a.date - b.date),
    };
    const companies = raw.companies.map((c) => ({
      ...c,
      start: parseMonth(c.start),
      end: parseMonth(c.end),
    }));
    const industries = (raw.industries || []).map((c) => ({
      ...c,
      start: parseMonth(c.start),
      end: parseMonth(c.end),
    }));
    const clients = raw.clients.map((c) => ({
      ...c,
      start: parseMonth(c.start),
      end: parseMonth(c.end),
    }));
    const techCategories = raw.tech_categories.map((cat) => ({
      name: cat.name,
      items: cat.items.map((it) => ({
        name: it.name,
        ranges: it.ranges.map((r) => ({
          start: parseMonth(r.start),
          end: parseMonth(r.end),
        })),
      })),
    }));

    const companyColor = d3
      .scaleOrdinal()
      .domain(companies.map((c) => c.name))
      .range(d3.schemeTableau10);

    const sections = [
      { id: 'seniority', label: 'Seniority' },
      { id: 'companies', label: 'Companies' },
      { id: 'industries', label: 'Industries' },
      { id: 'clients', label: 'Clients' },
      ...techCategories.map((cat) => ({
        id: `tech:${cat.name}`,
        label: cat.name,
        kind: 'tech',
        category: cat,
      })),
    ];
    const collapsedByDefault = new Set(['clients', 'industries']);
    const expanded = new Set(
      sections.filter((s) => !collapsedByDefault.has(s.id)).map((s) => s.id),
    );

    let cursorDate = null;
    const scrollContainer = svg.closest('.career-arc-scroll') || svg.parentElement;
    // 2400px gives ~7px/month across 29 years — enough breathing
    // room that 4-month tenures (Allied Steel) carry a readable
    // label inside their band.
    const minWidth = 2400;
    let panRafId = null;
    let panVelocity = 0;
    let firstRender = true;
    // Rows that get relevance-dimmed (tech + client). Each entry
    // points at the row's label + content elements so the scroll
    // handler can flip opacity without re-rendering.
    let dimmableRows = [];

    function render() {
      const containerWidth = scrollContainer
        ? scrollContainer.clientWidth
        : svg.clientWidth;
      const width = Math.max(minWidth, containerWidth);
      const margin = { top: 8, right: 16, bottom: 24, left: 132 };
      const headerH = 22;
      const sectionGap = 6;
      const techRowH = 12;
      const seniorityContentH = 130;
      const companiesContentH = 26;
      const industriesContentH = 26;
      const clientsContentH = clients.length * 14 + 4;

      let y = margin.top;
      const layout = sections.map((s) => {
        const isOpen = expanded.has(s.id);
        let contentH = 0;
        if (s.id === 'seniority') {
          contentH = seniorityContentH;
        } else if (s.id === 'companies') {
          contentH = companiesContentH;
        } else if (s.id === 'industries') {
          contentH = industriesContentH;
        } else if (s.id === 'clients') {
          contentH = clientsContentH;
        } else if (s.kind === 'tech') {
          contentH = s.category.items.length * techRowH + 4;
        }
        const headerY = y;
        const contentY = headerY + headerH;
        const sectionTotal = headerH + (isOpen ? contentH : 0);
        y += sectionTotal + sectionGap;
        return { section: s, headerY, contentY, contentH, isOpen };
      });

      const axisGap = 4;
      const axisH = 20;
      const height = y - sectionGap + axisGap + axisH + margin.bottom;

      svg.setAttribute('viewBox', `0 0 ${width} ${height}`);
      svg.setAttribute('width', width);
      svg.setAttribute('height', height);

      const root = d3.select(svg);
      root.selectAll('*').remove();
      dimmableRows = [];

      const x = d3
        .scaleTime()
        .domain([spanStart, spanEnd])
        .range([margin.left, width - margin.right]);

      // Shared year gridlines.
      const yearTicks = d3.timeYears(spanStart, spanEnd);
      const gridGroup = root.append('g');
      gridGroup
        .selectAll('line')
        .data(yearTicks)
        .enter()
        .append('line')
        .attr('x1', (d) => x(d))
        .attr('x2', (d) => x(d))
        .attr('y1', margin.top)
        .attr('y2', height - margin.bottom)
        .attr('stroke', 'currentColor')
        .attr('stroke-width', 1)
        .attr('opacity', 0.05);

      // Hit targets for section headers (full-width, transparent).
      // Live in main root so they stay anchored to their SVG
      // position regardless of scroll.
      const headerHitGroup = root.append('g');
      for (const item of layout) {
        renderSectionHitTarget(headerHitGroup, item, width, margin);
        renderSectionUnderline(root, item, width, margin);
        if (item.isOpen) {
          renderSectionContent(root, item, width, height, margin, x);
        }
      }

      // X axis along the bottom.
      const xAxisGroup = root
        .append('g')
        .attr('transform', `translate(0,${height - margin.bottom + 4})`)
        .attr('class', 'ca-muted');
      const tickCount = Math.max(6, Math.floor(width / 220));
      xAxisGroup.call(
        d3
          .axisBottom(x)
          .ticks(tickCount)
          .tickFormat(d3.timeFormat('%Y'))
          .tickSizeOuter(0),
      );

      // Hover guide line.
      const guide = root
        .append('line')
        .attr('y1', margin.top)
        .attr('y2', height - margin.bottom)
        .attr('stroke', 'currentColor')
        .attr('stroke-width', 1)
        .attr('opacity', 0)
        .attr('pointer-events', 'none');

      // Sticky-label group: rendered last so it's on top of
      // section content but below the hover overlay. Translated
      // by `scrollLeft` in the scroll handler so labels stay
      // pinned to the visible left edge as the reader pans.
      const labelGroup = root
        .append('g')
        .attr('class', 'sticky-labels')
        .attr('pointer-events', 'none');
      // Background mask so chart content doesn't bleed through
      // behind the labels when the container is scrolled.
      labelGroup
        .append('rect')
        .attr('x', 0)
        .attr('y', 0)
        .attr('width', margin.left)
        .attr('height', height)
        .attr('class', 'ca-bg');
      // Faint vertical divider between label rail and plot area.
      labelGroup
        .append('line')
        .attr('x1', margin.left - 0.5)
        .attr('x2', margin.left - 0.5)
        .attr('y1', margin.top)
        .attr('y2', height - margin.bottom)
        .attr('stroke', 'currentColor')
        .attr('opacity', 0.15);

      for (const item of layout) {
        renderSectionHeaderLabel(labelGroup, item, margin);
        if (item.isOpen) {
          renderSectionContentLabels(labelGroup, item, margin, x);
        }
      }

      // Apply the current scroll translate so labels start in the
      // right place on this render (e.g. after expand/collapse).
      applyStickyTransform();

      // Capture overlay for hover + auto-pan. Top of stack so it
      // intercepts pointer events.
      const overlay = root
        .append('rect')
        .attr('x', margin.left)
        .attr('y', margin.top)
        .attr('width', width - margin.left - margin.right)
        .attr('height', height - margin.top - margin.bottom)
        .attr('fill', 'transparent');
      overlay
        .on('mousemove touchmove', (event) => {
          event.preventDefault();
          const [px] = d3.pointer(event);
          const date = x.invert(px);
          if (date < spanStart || date > spanEnd) {
            return;
          }
          cursorDate = date;
          guide.attr('x1', px).attr('x2', px).attr('opacity', 0.4);
          updatePanel(date);
          updateAutoPan(event);
        })
        .on('mouseleave touchend', () => {
          cursorDate = null;
          guide.attr('opacity', 0);
          updatePanel(spanEnd);
          stopAutoPan();
        });

      updatePanel(cursorDate || spanEnd);
      currentMargin = margin;
      currentWidth = width;
      currentX = x;

      if (firstRender) {
        // Anchor first paint to the right edge: most recent year
        // visible, history is what you pan to reach.
        firstRender = false;
        // requestAnimationFrame so the browser has resolved the
        // SVG's natural scrollWidth before we measure.
        requestAnimationFrame(() => {
          if (scrollContainer) {
            scrollContainer.scrollLeft =
              scrollContainer.scrollWidth - scrollContainer.clientWidth;
            applyStickyTransform();
            updateRelevance();
          }
        });
      } else {
        // Re-apply sticky transform + relevance on resize / toggle
        // so labels track the new layout without a visible jump.
        applyStickyTransform();
        updateRelevance();
      }
    }

    // Cached after each render so the scroll handler doesn't have
    // to recompute the X scale on every frame.
    let currentMargin = null;
    let currentWidth = null;
    let currentX = null;

    function applyStickyTransform() {
      const scrollLeft = scrollContainer ? scrollContainer.scrollLeft : 0;
      d3.select(svg)
        .select('.sticky-labels')
        .attr('transform', `translate(${scrollLeft},0)`);
    }

    // Dim rows whose date ranges don't intersect the currently
    // visible window. The window is the chart's plot area as it
    // appears in the scroll container's viewport — translated
    // through the X scale to dates. Threshold is 30% opacity for
    // out-of-window rows; instant rather than transitioned so
    // scroll feels live, not laggy.
    function updateRelevance() {
      if (!currentX || !scrollContainer) {
        return;
      }
      const scrollLeft = scrollContainer.scrollLeft;
      const clientWidth = scrollContainer.clientWidth;
      const margin = currentMargin;
      const width = currentWidth;
      const visibleX0 = Math.max(scrollLeft, margin.left);
      const visibleX1 = Math.min(scrollLeft + clientWidth, width - margin.right);
      if (visibleX1 <= visibleX0) {
        return;
      }
      const visibleStart = currentX.invert(visibleX0);
      const visibleEnd = currentX.invert(visibleX1);
      const startMs = visibleStart.getTime();
      const endMs = visibleEnd.getTime();
      for (const row of dimmableRows) {
        const overlaps = row.ranges.some(
          (r) => r.start.getTime() < endMs && r.end.getTime() > startMs,
        );
        const op = overlaps ? 1 : 0.3;
        for (const el of row.elements) {
          d3.select(el).attr('opacity', op);
        }
      }
    }

    function updateAutoPan(event) {
      if (!scrollContainer) {
        return;
      }
      const rect = scrollContainer.getBoundingClientRect();
      const cursorX = event.clientX - rect.left;
      const PAN_ZONE = 80;
      const MAX_SPEED = 14;
      let velocity = 0;
      if (cursorX < PAN_ZONE) {
        velocity = -((PAN_ZONE - cursorX) / PAN_ZONE) * MAX_SPEED;
      } else if (cursorX > rect.width - PAN_ZONE) {
        const excess = cursorX - (rect.width - PAN_ZONE);
        velocity = (excess / PAN_ZONE) * MAX_SPEED;
      }
      panVelocity = velocity;
      if (velocity !== 0 && panRafId === null) {
        const tick = () => {
          if (panVelocity === 0) {
            panRafId = null;
            return;
          }
          scrollContainer.scrollLeft += panVelocity;
          panRafId = requestAnimationFrame(tick);
        };
        panRafId = requestAnimationFrame(tick);
      }
    }

    function stopAutoPan() {
      panVelocity = 0;
      if (panRafId !== null) {
        cancelAnimationFrame(panRafId);
        panRafId = null;
      }
    }

    function renderSectionHitTarget(parent, item, width, margin) {
      const g = parent
        .append('g')
        .attr('tabindex', 0)
        .attr('role', 'button')
        .attr(
          'aria-label',
          `${item.isOpen ? 'Collapse' : 'Expand'} ${item.section.label}`,
        )
        .attr('aria-expanded', item.isOpen ? 'true' : 'false')
        .style('cursor', 'pointer');
      g.append('rect')
        .attr('x', 0)
        .attr('y', item.headerY)
        .attr('width', width)
        .attr('height', 22)
        .attr('fill', 'transparent');
      g.on('click', () => toggle(item.section.id));
      g.on('keydown', (event) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          toggle(item.section.id);
        }
      });
    }

    function renderSectionUnderline(parent, item, width, margin) {
      parent
        .append('line')
        .attr('x1', margin.left)
        .attr('x2', width - margin.right)
        .attr('y1', item.headerY + 22 - 1)
        .attr('y2', item.headerY + 22 - 1)
        .attr('stroke', 'currentColor')
        .attr('opacity', 0.12);
    }

    function renderSectionHeaderLabel(labelGroup, item, margin) {
      labelGroup
        .append('text')
        .attr('x', 8)
        .attr('y', item.headerY + 15)
        .attr('class', 'ca-muted')
        .style('font-size', '10px')
        .style('font-family', 'monospace')
        .text(item.isOpen ? '▼' : '▶');
      labelGroup
        .append('text')
        .attr('x', 22)
        .attr('y', item.headerY + 15)
        .attr('class', 'ca-text')
        .style('font-size', '11px')
        .style('font-weight', '600')
        .style('letter-spacing', '0.04em')
        .style('text-transform', 'uppercase')
        .text(item.section.label);
    }

    function renderSectionContent(root, item, width, height, margin, x) {
      const top = item.contentY;
      const bottom = item.contentY + item.contentH;
      if (item.section.id === 'seniority') {
        renderSeniority(root, top, bottom, width, margin, x);
      } else if (item.section.id === 'companies') {
        renderCompanies(root, top, bottom, width, margin, x);
      } else if (item.section.id === 'industries') {
        renderIndustries(root, top, bottom, width, margin, x);
      } else if (item.section.id === 'clients') {
        renderClients(root, top, bottom, width, margin, x);
      } else if (item.section.kind === 'tech') {
        renderTechCategory(
          root,
          item.section.category,
          top,
          bottom,
          width,
          margin,
          x,
        );
      }
    }

    function renderSectionContentLabels(labelGroup, item, margin, x) {
      const top = item.contentY;
      const bottom = item.contentY + item.contentH;
      if (item.section.id === 'seniority') {
        renderSeniorityLabels(labelGroup, top, bottom, margin);
      } else if (item.section.id === 'clients') {
        renderClientLabels(labelGroup, top, bottom, margin);
      } else if (item.section.kind === 'tech') {
        renderTechCategoryLabels(
          labelGroup,
          item.section.category,
          top,
          bottom,
          margin,
        );
      }
      // Companies & industries draw their text inside their bars,
      // which travels with the chart — not sticky.
    }

    // Three parallel lanes — Manager (top), Lead (middle), IC
    // (bottom). Each lane is its own visual territory: the IC
    // lane has internal Junior/Senior/Principal sub-positions;
    // Lead and Manager are single-row lanes. A connected step
    // line moves through the lanes; track changes appear as
    // vertical jumps between lanes. Y position within a lane is
    // only meaningful inside that lane — never compared across
    // lanes, since the tracks are parallel careers, not points
    // on a shared scale.
    //
    // Lane order: Manager on top (org-chart convention), Lead
    // middle, IC on bottom. Within IC, Principal is at the top of
    // the lane and Junior at the bottom so the most senior IC
    // position sits adjacent to the Lead lane — visually
    // suggesting the rough equivalence of Principal IC and Lead+
    // without asserting they're points on one ladder.
    function seniorityLanes(top, bottom) {
      const total = bottom - top;
      const gap = 8;
      // Manager and Lead are single-position rows; IC has three
      // sub-positions and gets a taller share. Proportions:
      // IC=4, Lead=2, Manager=2, gaps=2 (×2 = 4). Total = 12.
      const unit = (total - 2 * gap) / 8;
      const mgrTop = top;
      const mgrBottom = mgrTop + 2 * unit;
      const leadTop = mgrBottom + gap;
      const leadBottom = leadTop + 2 * unit;
      const icTop = leadBottom + gap;
      const icBottom = icTop + 4 * unit;
      return {
        Manager: { top: mgrTop, bottom: mgrBottom },
        Lead: { top: leadTop, bottom: leadBottom },
        IC: { top: icTop, bottom: icBottom },
      };
    }

    // Map (track, level) → Y coordinate within the right lane.
    // For IC, level chooses one of three positions inside the
    // lane (Junior near the bottom edge, Principal near the
    // top). For Lead and Manager the single position is the lane
    // center.
    function laneY(lanes, track, level) {
      const lane = lanes[track];
      if (!lane) return lanes.IC.bottom;
      if (track !== 'IC') {
        return (lane.top + lane.bottom) / 2;
      }
      const sublevels = ['Junior', 'Senior', 'Principal'];
      const idx = sublevels.indexOf(level);
      const safeIdx = idx < 0 ? 0 : idx;
      // 3 evenly-spaced positions with a small inset from the
      // lane edges.
      const inset = 4;
      const usable = lane.bottom - lane.top - inset * 2;
      const fraction = safeIdx / (sublevels.length - 1);
      return lane.bottom - inset - fraction * usable;
    }

    function renderSeniority(root, top, bottom, width, margin, x) {
      const lanes = seniorityLanes(top, bottom);
      const g = root.append('g');

      // Faint lane backgrounds — barely visible tints so the
      // lanes register as territory without competing with the
      // line.
      const laneOrder = ['Manager', 'Lead', 'IC'];
      g.selectAll('rect.lane-bg')
        .data(laneOrder)
        .enter()
        .append('rect')
        .attr('class', 'lane-bg')
        .attr('x', margin.left)
        .attr('y', (d) => lanes[d].top)
        .attr('width', width - margin.left - margin.right)
        .attr('height', (d) => lanes[d].bottom - lanes[d].top)
        .attr('fill', 'currentColor')
        .attr('opacity', 0.04);

      // Faint sub-level guides inside the IC lane only.
      const icLane = lanes.IC;
      const icLevels = ['Junior', 'Senior', 'Principal'];
      g.selectAll('line.ic-sub')
        .data(icLevels)
        .enter()
        .append('line')
        .attr('class', 'ic-sub')
        .attr('x1', margin.left)
        .attr('x2', width - margin.right)
        .attr('y1', (d) => laneY(lanes, 'IC', d))
        .attr('y2', (d) => laneY(lanes, 'IC', d))
        .attr('stroke', 'currentColor')
        .attr('opacity', 0.06);

      // Build the step path: one continuous polyline that jumps
      // between lanes at track transitions.
      const points = [];
      for (let i = 0; i < seniority.transitions.length; i++) {
        const cur = seniority.transitions[i];
        const next = seniority.transitions[i + 1];
        const segEnd = next ? next.date : spanEnd;
        const y = laneY(lanes, cur.track, cur.level);
        points.push({ date: cur.date, y });
        points.push({ date: segEnd, y });
      }
      const lineGen = d3
        .line()
        .x((d) => x(d.date))
        .y((d) => d.y)
        .curve(d3.curveStepAfter);
      g.append('path')
        .datum(points)
        .attr('fill', 'none')
        .attr('class', 'ca-text')
        .attr('stroke-width', 2)
        .attr('opacity', 0.9)
        .attr('d', lineGen);

      // Transition markers at each (date, lane Y) pair.
      g.selectAll('circle.transition')
        .data(seniority.transitions)
        .enter()
        .append('circle')
        .attr('cx', (d) => x(d.date))
        .attr('cy', (d) => laneY(lanes, d.track, d.level))
        .attr('r', 3.5)
        .attr('class', 'ca-text');
    }

    function renderSeniorityLabels(labelGroup, top, bottom, margin) {
      const lanes = seniorityLanes(top, bottom);

      // Manager and Lead lanes get a single bold lane label
      // centered vertically. The IC lane is labeled by its
      // sub-levels (Principal/Senior/Junior) — a small "IC"
      // marker sits above the topmost sub-level so the lane's
      // track identity is explicit without crowding the
      // per-level labels.
      labelGroup
        .append('text')
        .attr('class', 'ca-text')
        .attr('x', margin.left - 8)
        .attr('y', (lanes.Manager.top + lanes.Manager.bottom) / 2 + 4)
        .attr('text-anchor', 'end')
        .style('font-size', '11px')
        .style('font-weight', '500')
        .text('Manager');
      labelGroup
        .append('text')
        .attr('class', 'ca-text')
        .attr('x', margin.left - 8)
        .attr('y', (lanes.Lead.top + lanes.Lead.bottom) / 2 + 4)
        .attr('text-anchor', 'end')
        .style('font-size', '11px')
        .style('font-weight', '500')
        .text('Lead');
      labelGroup
        .append('text')
        .attr('class', 'ca-muted')
        .attr('x', margin.left - 8)
        .attr('y', lanes.IC.top - 4)
        .attr('text-anchor', 'end')
        .style('font-size', '9px')
        .style('text-transform', 'uppercase')
        .style('letter-spacing', '0.06em')
        .text('IC');

      // IC sub-level labels.
      const icSubs = [
        { name: 'Principal', level: 'Principal' },
        { name: 'Senior', level: 'Senior' },
        { name: 'Junior', level: 'Junior' },
      ];
      labelGroup
        .selectAll('text.ic-sub-label')
        .data(icSubs)
        .enter()
        .append('text')
        .attr('class', 'ca-text ic-sub-label')
        .attr('x', margin.left - 8)
        .attr('y', (d) => laneY(lanes, 'IC', d.level) + 3)
        .attr('text-anchor', 'end')
        .style('font-size', '10px')
        .text((d) => d.name);
    }

    function renderCompanies(root, top, bottom, width, margin, x) {
      const g = root.append('g');
      const rowH = bottom - top - 4;
      g.selectAll('rect')
        .data(companies)
        .enter()
        .append('rect')
        .attr('x', (d) => x(d.start))
        .attr('y', top + 2)
        .attr('width', (d) => Math.max(2, x(d.end) - x(d.start)))
        .attr('height', rowH)
        .attr('rx', 2)
        .attr('fill', (d) => companyColor(d.name))
        .attr('opacity', 0.72);
      g.selectAll('text')
        .data(companies)
        .enter()
        .append('text')
        .attr('x', (d) => x(d.start) + 6)
        .attr('y', top + 2 + rowH / 2 + 4)
        .attr('class', 'ca-strong pointer-events-none')
        .style('font-size', '11px')
        .style('font-weight', '500')
        .text((d) => {
          const w = x(d.end) - x(d.start);
          if (w < 40) return '';
          if (w < 100) return shortCompany(d.name);
          return d.name;
        });
    }

    function renderIndustries(root, top, bottom, width, margin, x) {
      const g = root.append('g');
      const rowH = bottom - top - 4;
      g.selectAll('rect')
        .data(industries)
        .enter()
        .append('rect')
        .attr('x', (d) => x(d.start))
        .attr('y', top + 2)
        .attr('width', (d) => Math.max(2, x(d.end) - x(d.start)))
        .attr('height', rowH)
        .attr('rx', 2)
        .attr('fill', 'currentColor')
        .attr('opacity', 0.12)
        .attr('stroke', 'currentColor')
        .attr('stroke-opacity', 0.25)
        .attr('stroke-width', 1);
      g.selectAll('text')
        .data(industries)
        .enter()
        .append('text')
        .attr('x', (d) => x(d.start) + 6)
        .attr('y', top + 2 + rowH / 2 + 4)
        .attr('class', 'ca-text pointer-events-none')
        .style('font-size', '10.5px')
        .text((d) => {
          const w = x(d.end) - x(d.start);
          if (w < 40) return '';
          if (w < 130) return d.name.split(/[\s/]/)[0];
          return d.name;
        });
    }

    function renderClients(root, top, bottom, width, margin, x) {
      const g = root.append('g');
      const rowH = 12;
      clients.forEach((c, i) => {
        const rowY = top + 2 + i * 14;
        const rect = g.append('rect')
          .attr('x', x(c.start))
          .attr('y', rowY)
          .attr('width', Math.max(2, x(c.end) - x(c.start)))
          .attr('height', rowH)
          .attr('rx', 2)
          .attr('fill', companyColor(c.via))
          .attr('opacity', 0.55);
        // Stash for the dimming pass — label element is added in
        // renderClientLabels and registered there.
        c._rect = rect.node();
      });
    }

    function renderClientLabels(labelGroup, top, bottom, margin) {
      const rowH = 12;
      clients.forEach((c, i) => {
        const rowY = top + 2 + i * 14;
        const label = labelGroup
          .append('text')
          .attr('x', margin.left - 8)
          .attr('y', rowY + rowH - 2)
          .attr('text-anchor', 'end')
          .attr('class', 'ca-text')
          .style('font-size', '10px')
          .text(c.name);
        const elements = [label.node()];
        if (c._rect) {
          elements.push(c._rect);
        }
        dimmableRows.push({
          ranges: [{ start: c.start, end: c.end }],
          elements,
        });
      });
    }

    function renderTechCategory(root, cat, top, bottom, width, margin, x) {
      const g = root.append('g');
      cat.items.forEach((item, i) => {
        const rowY = top + 2 + i * 12;
        const rowH = 10;
        // Row baseline.
        g.append('line')
          .attr('x1', margin.left)
          .attr('x2', width - margin.right)
          .attr('y1', rowY + rowH / 2)
          .attr('y2', rowY + rowH / 2)
          .attr('stroke', 'currentColor')
          .attr('opacity', 0.05);
        // Stripes group — opacity-dimmed as a unit.
        const stripeGroup = g.append('g');
        for (const range of item.ranges) {
          stripeGroup
            .append('rect')
            .attr('x', x(range.start))
            .attr('y', rowY + 1)
            .attr('width', Math.max(2, x(range.end) - x(range.start)))
            .attr('height', rowH - 2)
            .attr('rx', 1.5)
            .attr('fill', 'currentColor')
            .attr('opacity', 0.55);
        }
        item._stripeGroup = stripeGroup.node();
      });
    }

    function renderTechCategoryLabels(labelGroup, cat, top, bottom, margin) {
      cat.items.forEach((item, i) => {
        const rowY = top + 2 + i * 12;
        const rowH = 10;
        const label = labelGroup
          .append('text')
          .attr('x', margin.left - 8)
          .attr('y', rowY + rowH - 1)
          .attr('text-anchor', 'end')
          .attr('class', 'ca-text')
          .style('font-size', '10px')
          .text(item.name);
        const elements = [label.node()];
        if (item._stripeGroup) {
          elements.push(item._stripeGroup);
        }
        dimmableRows.push({ ranges: item.ranges, elements });
      });
    }

    function toggle(id) {
      if (expanded.has(id)) {
        expanded.delete(id);
      } else {
        expanded.add(id);
      }
      render();
    }

    function updatePanel(date) {
      panel.innerHTML = '';
      const slice = sliceAt(date);
      const meta = document.createElement('div');
      meta.className =
        'text-xs font-medium uppercase tracking-wide text-stone-500 dark:text-stone-500';
      meta.textContent = date.toLocaleString('en-US', {
        month: 'short',
        year: 'numeric',
        timeZone: 'UTC',
      });
      panel.appendChild(meta);

      const headerRow = document.createElement('div');
      headerRow.className = 'mt-1 flex flex-wrap items-baseline gap-x-3 gap-y-1';
      const seniorityEl = document.createElement('span');
      seniorityEl.className = 'text-xl font-semibold tracking-tight';
      // Render the seniority as a compact "Track · Level" string
      // when the level is meaningful (IC and Lead tracks); fall
      // back to the bare track name otherwise (Manager).
      seniorityEl.textContent = slice.level
        ? `${slice.level} ${slice.track}`
        : (slice.track || 'Unknown');
      headerRow.appendChild(seniorityEl);
      if (slice.company) {
        const companyEl = document.createElement('span');
        companyEl.className = 'text-stone-700 dark:text-stone-300';
        companyEl.textContent = `@ ${slice.company}`;
        headerRow.appendChild(companyEl);
      }
      panel.appendChild(headerRow);

      if (slice.clients.length) {
        const c = document.createElement('p');
        c.className = 'mt-2 text-sm text-stone-600 dark:text-stone-400';
        c.textContent = `Clients: ${slice.clients.join(', ')}`;
        panel.appendChild(c);
      }

      if (slice.techByCategory.size) {
        const grid = document.createElement('dl');
        grid.className =
          'mt-3 grid grid-cols-1 sm:grid-cols-2 gap-x-6 gap-y-2 text-sm';
        for (const [cat, techs] of slice.techByCategory) {
          const dt = document.createElement('dt');
          dt.className =
            'text-xs font-medium uppercase tracking-wide text-stone-500 dark:text-stone-500 sm:col-span-2';
          dt.textContent = cat;
          grid.appendChild(dt);
          const dd = document.createElement('dd');
          dd.className =
            'sm:col-span-2 flex flex-wrap gap-1.5 text-stone-700 dark:text-stone-300';
          for (const t of techs) {
            const li = document.createElement('span');
            li.className =
              'inline-flex items-center rounded-full bg-stone-200 px-2 py-0.5 text-xs dark:bg-stone-800';
            li.textContent = t;
            dd.appendChild(li);
          }
          grid.appendChild(dd);
        }
        panel.appendChild(grid);
      }
    }

    function sliceAt(date) {
      const ms = date.getTime();
      let track = seniority.transitions[0]?.track || null;
      let level = seniority.transitions[0]?.level || null;
      for (const t of seniority.transitions) {
        if (t.date.getTime() <= ms) {
          track = t.track;
          level = t.level;
        }
      }
      const company = companies.find(
        (c) => c.start.getTime() <= ms && c.end.getTime() > ms,
      );
      const activeClients = clients
        .filter((c) => c.start.getTime() <= ms && c.end.getTime() > ms)
        .map((c) => c.name);
      const techByCategory = new Map();
      for (const cat of techCategories) {
        const active = [];
        for (const item of cat.items) {
          if (
            item.ranges.some(
              (r) => r.start.getTime() <= ms && r.end.getTime() > ms,
            )
          ) {
            active.push(item.name);
          }
        }
        if (active.length) {
          techByCategory.set(cat.name, active);
        }
      }
      return {
        track,
        level,
        company: company?.name || null,
        clients: activeClients,
        techByCategory,
      };
    }

    render();

    // Single scroll listener (registered once). Throttles via rAF
    // so high-frequency scroll events coalesce into one update
    // per frame.
    if (scrollContainer) {
      let scrollRaf = null;
      scrollContainer.addEventListener('scroll', () => {
        if (scrollRaf !== null) {
          return;
        }
        scrollRaf = requestAnimationFrame(() => {
          scrollRaf = null;
          applyStickyTransform();
          updateRelevance();
        });
      });
    }

    let resizeTimer = null;
    window.addEventListener('resize', () => {
      clearTimeout(resizeTimer);
      resizeTimer = setTimeout(render, 100);
    });
  }

  function parseMonth(s) {
    return new Date(`${s}-01T00:00:00Z`);
  }

  function shortCompany(name) {
    const words = name.split(/[\s/]/);
    return words[0];
  }
})();
