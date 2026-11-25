ALTER TABLE paper_orders
    ADD COLUMN scout_pick_id UUID REFERENCES scout_picks(id),
    ADD CONSTRAINT paper_orders_one_signal_source
        CHECK (signal_id IS NULL OR scout_pick_id IS NULL);
