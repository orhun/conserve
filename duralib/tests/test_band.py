# Copyright 2012 Martin Pool
# Licensed under the Apache License, Version 2.0 (the "License").

"""Unit test bands"""


from __future__ import absolute_import

import unittest

from duralib.band import (
    Band,
    canonical_band_number,
    cmp_band_numbers,
    )
from duralib.tests.base import DuraTestCase
from duralib.tests.durafixtures import (
    EmptyArchive,
    )


class TestBand(DuraTestCase):

    def test_band_repr(self):
        archive = self.useFixture(EmptyArchive()).archive
        band = archive.create_band()
        self.assertEquals(
            "BandWriter(path='%s')" % band.path,
            repr(band))


class TestBandNumbers(DuraTestCase):
    """Test formatting, parsing, sorting of band numbers."""

    def test_match_band_name(self):
        self.assertEqual("0000", Band.match_band_name("b0000"))
        self.assertEqual("0042", Band.match_band_name("b0042"))
        self.assertEqual("420000", Band.match_band_name("b420000"))
        self.assertEqual(None, Band.match_band_name("pony"))

    def test_canonical_band_number(self):
        self.assertEqual("0000", canonical_band_number("0"))
        self.assertEqual("0042", canonical_band_number("42"))
        self.assertEqual("9999", canonical_band_number("9999"))
        self.assertEqual("123456", canonical_band_number("123456"))

    def test_cmp_band_number(self):
        self.assertEqual(-1, cmp_band_numbers("0000", "0001"))
        self.assertEqual(1, cmp_band_numbers("0900", "0001"))
        self.assertEqual(0, cmp_band_numbers("0900", "900"))
        self.assertEqual(-1, cmp_band_numbers("9000", "10001"))

    def test_sort_band_number(self):
        # Smart comparison, by number.
        numbers = ["0000", "0001", "0042", "9998", "9999", "10000", "12345",
        "990099"]
        self.assertEqual(
            numbers,
            sorted(numbers, cmp=cmp_band_numbers))
        self.assertEqual(
            numbers,
            sorted(sorted(numbers), cmp=cmp_band_numbers))
        self.assertEqual(
            numbers,
            sorted(sorted(numbers, reverse=True),
                cmp=cmp_band_numbers))


class TestBandHead(DuraTestCase):

    def test_read_head(self):
        archive = self.useFixture(EmptyArchive()).archive
        writer = archive.create_band()
        self.assertFalse(writer.is_finished())
        # try to read while it's open
        reader = archive.open_band_reader(writer.band_number)
        self.assertFalse(reader.is_finished())
        self.assertEquals("0000", reader.band_number)
        # finish, and try to read again
        writer.finish_band()
        reader = archive.open_band_reader(writer.band_number)
        self.assertEquals("0000", reader.band_number)
        self.assertTrue(reader.is_finished())
        self.assertTrue(writer.is_finished())


class TestBandBlocks(DuraTestCase):

    def test_list_blocks_empty(self):
        archive = self.useFixture(EmptyArchive()).archive
        writer = archive.create_band()
        self.assertEquals([], writer.list_blocks())

    def test_next_block_empty(self):
        archive = self.useFixture(EmptyArchive()).archive
        writer = archive.create_band()
        self.assertEquals('000000', writer.next_block_number())


if __name__ == '__main__':
    unittest.main()
