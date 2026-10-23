import math
import unittest
from game.tools.demo.assertions import TraceAssertions


def checker(**limits):
    check = dict(path='actor', window_seconds=2, max_turn_degrees=360,
                 max_rate_degrees=180, max_travel_turn_degrees=360)
    check.update(limits)
    return TraceAssertions(dict(version=1, motion=[check], final=[dict(path='delivered', min=1)]))


def frame(n, angle=0, position=None, delivered=1):
    return dict(frame=n, time=n / 10, state=dict(actor=dict(heading=math.radians(angle),
                position=position or [0, 0]), delivered=delivered))


class AssertionsTest(unittest.TestCase):
    def test_wraparound(self):
        c = checker()
        c.feed(frame(0, 179))
        c.feed(frame(1, -179))
        self.assertAlmostEqual(c.finish()['peaks']['actor:max_rate_degrees'], 20)

    def test_turnaround(self):
        c = checker()
        for i in range(11):
            c.feed(frame(i, i * 18))
        c.finish()

    def test_rapid_flip(self):
        c = checker()
        c.feed(frame(0))
        with self.assertRaisesRegex(AssertionError, 'frame 1.*max_rate_degrees'):
            c.feed(frame(1, 180))

    def test_repeated_slow_turns(self):
        c = checker(max_turn_degrees=90)
        with self.assertRaisesRegex(AssertionError, 'max_turn_degrees'):
            for i in range(20):
                c.feed(frame(i, (i % 2) * 15))

    def test_smoothed_facing_does_not_hide_position_jitter(self):
        c = checker()
        with self.assertRaisesRegex(AssertionError, 'max_travel_turn_degrees'):
            for i in range(10):
                c.feed(frame(i, position=[i % 2, 0]))

    def test_window_expires(self):
        c = checker(max_turn_degrees=180)
        for i in range(50):
            c.feed(frame(i, min(i, 10) * 18 if i < 30 else max(0, 40 - i) * 18))
        c.finish()

    def test_empty_and_frozen_do_not_pass(self):
        with self.assertRaises(AssertionError):
            checker().finish()
        c = checker()
        c.feed(frame(0, delivered=0))
        c.feed(frame(1, delivered=0))
        with self.assertRaisesRegex(AssertionError, 'delivered'):
            c.finish()

    def test_missing_actor_and_bad_time(self):
        c = checker()
        with self.assertRaises(KeyError):
            c.feed(dict(frame=0, time=0, state={}))
        for time in (float('nan'), -1, 0):
            c = checker()
            c.feed(frame(0))
            with self.assertRaises(AssertionError):
                c.feed(dict(frame=1, time=time, state=frame(1)['state']))

    def test_always(self):
        c = TraceAssertions(dict(version=1, always=[dict(path='delivered', max=3)]))
        with self.assertRaisesRegex(AssertionError, 'delivered'):
            c.feed(frame(0, delivered=4))

    def test_eventual_nested_state_and_collection_size(self):
        spec = dict(version=1, ever=[
            dict(path='threats.*.pierced', contains=True),
            dict(path='pinned', min_len=1),
        ])
        c = TraceAssertions(spec)
        c.feed(dict(frame=0, time=0., state=dict(threats={'a': {'pierced': False}}, pinned={})))
        c.feed(dict(frame=1, time=0.1, state=dict(threats={'a': {'pierced': True}}, pinned={'b': {}})))
        c.finish()
        missing = TraceAssertions(spec)
        missing.feed(dict(frame=0, time=0., state=dict(threats={}, pinned={})))
        missing.feed(dict(frame=1, time=0.1, state=dict(threats={}, pinned={})))
        with self.assertRaisesRegex(AssertionError, 'never satisfied'):
            missing.finish()


if __name__ == '__main__':
    unittest.main()
