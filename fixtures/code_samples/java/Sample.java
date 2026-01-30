package com.example;

public class Sample {
    private String name;
    private int value;

    public Sample(String name, int value) {
        this.name = name;
        this.value = value;
    }

    public String display() {
        return name + ": " + value;
    }

    public static int helper(int x) {
        return x * 2;
    }
}

interface Processor {
    String process(String input);
    boolean validate();
}

enum Status {
    ACTIVE,
    INACTIVE,
    PENDING
}
